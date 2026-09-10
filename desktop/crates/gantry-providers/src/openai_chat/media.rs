//! The models that do not answer on `chat/completions` (docs/plan/02 §4b).
//!
//! A model that draws a picture, reads a passage aloud or renders a clip is not a chat model
//! with an extra output modality: it takes a prompt, not a conversation, and it answers with a
//! file. OpenRouter puts each on an endpoint of its own — `POST /images`, `POST /audio/speech`,
//! `POST /videos` — and leaves all three out of the plain model list, which is why the catalog
//! asks for them by name (`?output_modality=…`).
//!
//! Gantry hides that split. The picker offers every kind, the composer sends a message, and
//! this module answers on the same [`ChatStream`] the chat client does, so a turn does not have
//! to know which kind of model it is talking to. What it cannot hide is time: a clip takes from
//! half a minute to several, so the video route reports what the job is doing while it waits.

use std::time::{Duration, Instant};

use base64::Engine;
use futures_util::stream;
use gantry_core::{ContentPart, MediaSource, StopReason, Usage};
use reqwest::header::HeaderMap;
use serde::Deserialize;

use crate::{
    error::ProviderError,
    http,
    provider::{ChatRequest, ChatStream, Modality, ModelInfo, StreamEvent},
};

/// The largest file Gantry will pull into a message. A clip of the length these models make is
/// comfortably inside it; anything past it is refused with its size named rather than being held
/// in memory, encoded, and sent through the interface.
pub const MAX_MEDIA_BYTES: usize = 32 * 1024 * 1024;

const IMAGE_TIMEOUT: Duration = Duration::from_secs(300);
const SPEECH_TIMEOUT: Duration = Duration::from_secs(300);
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
/// Often enough that the interface feels alive, far short of the rate a poll loop can annoy a
/// server with. The provider's own advice is 30 s; a person watching a spinner disagrees.
const POLL_INTERVAL: Duration = Duration::from_secs(10);
/// A job that has not finished in a quarter of an hour has gone wrong.
const VIDEO_DEADLINE: Duration = Duration::from_secs(15 * 60);

/// Which endpoint a model answers on, or `None` for the chat endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaRoute {
    Image,
    Speech,
    Video,
}

/// What a model's output modalities say about where to send it.
///
/// A model that answers with a picture *and* text is a chat model that happens to draw
/// (Gemini's image models are the case), and it keeps the chat endpoint. One that only draws
/// has no message list to send at all.
#[must_use]
pub fn route(info: Option<&ModelInfo>) -> Option<MediaRoute> {
    let out = &info?.capabilities.output;
    if out.contains(&Modality::Video) {
        return Some(MediaRoute::Video);
    }
    if out.contains(&Modality::Speech) {
        return Some(MediaRoute::Speech);
    }
    if out.contains(&Modality::Image) && !out.contains(&Modality::Text) {
        return Some(MediaRoute::Image);
    }
    None
}

/// The prompt: the last thing the user said. These endpoints take one string, so the history a
/// chat model would have read is not sent — and the alternative, pasting the whole conversation
/// into the prompt, would put the model's own past answers into its next picture.
#[must_use]
pub fn prompt(req: &ChatRequest) -> String {
    req.messages
        .iter()
        .rev()
        .find(|m| m.role == gantry_core::Role::User)
        .map(|m| m.text())
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct MediaUsage {
    cost: Option<f64>,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

impl MediaUsage {
    fn to_usage(&self) -> Usage {
        Usage {
            input: self.prompt_tokens.unwrap_or(0),
            output: self.completion_tokens.unwrap_or(0),
            cost_usd: self.cost,
            ..Usage::default()
        }
    }
}

#[derive(Debug, Deserialize)]
struct ImageReply {
    #[serde(default)]
    data: Vec<ImageDatum>,
    usage: Option<MediaUsage>,
}

#[derive(Debug, Deserialize)]
struct ImageDatum {
    b64_json: Option<String>,
    url: Option<String>,
    /// The provider spells this two ways depending on the upstream.
    media_type: Option<String>,
    mime_type: Option<String>,
}

/// `POST /images`: one prompt in, base64 pictures out.
pub async fn image(
    http: &reqwest::Client,
    base_url: &str,
    headers: HeaderMap,
    req: &ChatRequest,
) -> Result<ChatStream, ProviderError> {
    let body = serde_json::json!({ "model": req.model, "prompt": prompt(req) });
    let json = http::post_json(
        http,
        &http::join(base_url, "images"),
        headers.clone(),
        &body,
        IMAGE_TIMEOUT,
    )
    .await?;
    let reply: ImageReply = serde_json::from_value(json)
        .map_err(|e| ProviderError::interrupted(format!("malformed image reply: {e}")))?;

    let mut events = vec![StreamEvent::MessageStart {
        provider_message_id: None,
    }];
    let mut index = 0;
    for datum in &reply.data {
        let mime = datum
            .media_type
            .clone()
            .or_else(|| datum.mime_type.clone())
            .unwrap_or_else(|| "image/png".to_owned());
        let data = match (&datum.b64_json, &datum.url) {
            (Some(b64), _) => b64.clone(),
            (None, Some(url)) => {
                let (_, bytes) = http::get_bytes(
                    http,
                    url,
                    headers.clone(),
                    DOWNLOAD_TIMEOUT,
                    MAX_MEDIA_BYTES,
                )
                .await?;
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            }
            (None, None) => continue,
        };
        events.push(StreamEvent::ProviderBlock {
            index,
            part: ContentPart::Image {
                source: MediaSource::Base64 { data },
                mime,
            },
        });
        index += 1;
    }
    if index == 0 {
        return Err(ProviderError::interrupted(
            "the model answered without a picture".to_owned(),
        ));
    }
    if let Some(usage) = &reply.usage {
        events.push(StreamEvent::Usage(usage.to_usage()));
    }
    events.push(StreamEvent::MessageEnd {
        stop_reason: StopReason::EndTurn,
    });
    Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
}

/// `POST /audio/speech`: a passage in, spoken audio out.
///
/// The voice is the model's own first listed voice rather than a name Gantry chose: the lists
/// have nothing in common between vendors, and a wrong voice name is an error rather than a
/// different-sounding answer.
pub async fn speech(
    http: &reqwest::Client,
    base_url: &str,
    headers: HeaderMap,
    req: &ChatRequest,
    info: Option<&ModelInfo>,
) -> Result<ChatStream, ProviderError> {
    let text = prompt(req);
    let mut body = serde_json::json!({
        "model": req.model,
        "input": text,
        "response_format": "mp3",
    });
    if let Some(voice) = info.and_then(|i| i.capabilities.voices.first()) {
        body["voice"] = serde_json::Value::String(voice.clone());
    }
    let (mime, bytes) = http::post_bytes(
        http,
        &http::join(base_url, "audio/speech"),
        headers,
        &body,
        SPEECH_TIMEOUT,
        MAX_MEDIA_BYTES,
    )
    .await?;
    // An error comes back as JSON on the same endpoint that otherwise answers with a file.
    if mime.contains("json") || bytes.starts_with(b"{") {
        return Err(ProviderError::interrupted(format!(
            "the speech endpoint answered with a message rather than audio: {}",
            String::from_utf8_lossy(&bytes)
                .chars()
                .take(300)
                .collect::<String>()
        )));
    }
    let events = vec![
        StreamEvent::MessageStart {
            provider_message_id: None,
        },
        StreamEvent::ProviderBlock {
            index: 0,
            part: ContentPart::Audio {
                source: MediaSource::Base64 {
                    data: base64::engine::general_purpose::STANDARD.encode(&bytes),
                },
                mime: if mime.starts_with("audio/") {
                    mime
                } else {
                    "audio/mpeg".to_owned()
                },
            },
        },
        StreamEvent::MessageEnd {
            stop_reason: StopReason::EndTurn,
        },
    ];
    Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
}

#[derive(Debug, Deserialize)]
struct VideoReply {
    id: Option<String>,
    status: Option<String>,
    #[serde(default)]
    unsigned_urls: Vec<String>,
    error: Option<serde_json::Value>,
    usage: Option<MediaUsage>,
}

/// Where a video job has got to. Each step performs one await and yields one event, so
/// dropping the stream — which is what cancelling a turn does — stops the polling.
enum Phase {
    Submit,
    Poll { id: String },
    Fetch { id: String, url: Option<String> },
    Report { usage: Usage },
    End,
    Done,
}

struct VideoJob {
    http: reqwest::Client,
    base_url: String,
    headers: HeaderMap,
    model: String,
    prompt: String,
    phase: Phase,
    started: Instant,
    usage: Option<Usage>,
}

/// `POST /videos`, then poll until the clip is ready and download it.
#[must_use]
pub fn video(
    http: &reqwest::Client,
    base_url: &str,
    headers: HeaderMap,
    req: &ChatRequest,
) -> ChatStream {
    let job = VideoJob {
        http: http.clone(),
        base_url: base_url.to_owned(),
        headers,
        model: req.model.clone(),
        prompt: prompt(req),
        phase: Phase::Submit,
        started: Instant::now(),
        usage: None,
    };
    Box::pin(stream::unfold(job, |mut job| async move {
        job.step().await.map(|event| (event, job))
    }))
}

impl VideoJob {
    /// One await, one event. An error ends the stream: every phase after a failure would fail
    /// too, and the turn shows the reason.
    async fn step(&mut self) -> Option<Result<StreamEvent, ProviderError>> {
        let result = match std::mem::replace(&mut self.phase, Phase::Done) {
            Phase::Submit => self.submit().await,
            Phase::Poll { id } => self.poll(id).await,
            Phase::Fetch { id, url } => self.fetch(id, url).await,
            Phase::Report { usage } => {
                self.phase = Phase::End;
                Ok(StreamEvent::Usage(usage))
            }
            Phase::End => Ok(StreamEvent::MessageEnd {
                stop_reason: StopReason::EndTurn,
            }),
            Phase::Done => return None,
        };
        if result.is_err() {
            self.phase = Phase::Done;
        }
        Some(result)
    }

    async fn submit(&mut self) -> Result<StreamEvent, ProviderError> {
        let body = serde_json::json!({ "model": self.model, "prompt": self.prompt });
        let json = http::post_json(
            &self.http,
            &http::join(&self.base_url, "videos"),
            self.headers.clone(),
            &body,
            SUBMIT_TIMEOUT,
        )
        .await?;
        let reply: VideoReply = serde_json::from_value(json)
            .map_err(|e| ProviderError::interrupted(format!("malformed video reply: {e}")))?;
        let id = reply.id.ok_or_else(|| {
            ProviderError::interrupted("the video endpoint answered without a job id".to_owned())
        })?;
        self.phase = Phase::Poll { id };
        Ok(StreamEvent::MessageStart {
            provider_message_id: None,
        })
    }

    async fn poll(&mut self, id: String) -> Result<StreamEvent, ProviderError> {
        tokio::time::sleep(POLL_INTERVAL).await;
        if self.started.elapsed() > VIDEO_DEADLINE {
            return Err(ProviderError::interrupted(format!(
                "the clip was still not ready after {} minutes; the job is {id} at the provider",
                VIDEO_DEADLINE.as_secs() / 60
            )));
        }
        let json = http::get_json_with_timeout(
            &self.http,
            &http::join(&self.base_url, &format!("videos/{id}")),
            self.headers.clone(),
            POLL_TIMEOUT,
        )
        .await?;
        let reply: VideoReply = serde_json::from_value(json)
            .map_err(|e| ProviderError::interrupted(format!("malformed video status: {e}")))?;
        let status = reply.status.unwrap_or_default();
        self.usage = reply.usage.as_ref().map(MediaUsage::to_usage);
        match status.as_str() {
            "completed" | "succeeded" => {
                self.phase = Phase::Fetch {
                    url: reply.unsigned_urls.first().cloned(),
                    id,
                };
                Ok(notice("The clip is ready; fetching it."))
            }
            "failed" | "cancelled" | "expired" => Err(ProviderError::interrupted(format!(
                "the clip was not made: {status}{}",
                reply.error.map(|e| format!(" — {e}")).unwrap_or_default()
            ))),
            _ => {
                let seconds = self.started.elapsed().as_secs();
                self.phase = Phase::Poll { id };
                Ok(notice(&format!(
                    "Rendering — {seconds} s so far. Clips take from half a minute to several."
                )))
            }
        }
    }

    async fn fetch(
        &mut self,
        id: String,
        url: Option<String>,
    ) -> Result<StreamEvent, ProviderError> {
        // The listed URL is the one to use where there is one; the content route is the
        // documented fallback, and both want the key.
        let url = url
            .unwrap_or_else(|| http::join(&self.base_url, &format!("videos/{id}/content?index=0")));
        let (mime, bytes) = http::get_bytes(
            &self.http,
            &url,
            self.headers.clone(),
            DOWNLOAD_TIMEOUT,
            MAX_MEDIA_BYTES,
        )
        .await?;
        self.phase = match self.usage {
            Some(usage) => Phase::Report { usage },
            None => Phase::End,
        };
        Ok(StreamEvent::ProviderBlock {
            index: 0,
            part: ContentPart::Video {
                source: MediaSource::Base64 {
                    data: base64::engine::general_purpose::STANDARD.encode(&bytes),
                },
                mime: if mime.starts_with("video/") {
                    mime
                } else {
                    "video/mp4".to_owned()
                },
            },
        })
    }
}

fn notice(detail: &str) -> StreamEvent {
    StreamEvent::Notice {
        kind: "media_progress".to_owned(),
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use gantry_core::Message;

    use super::*;
    use crate::provider::{ModelCapabilities, ModelInfo};

    fn info(output: Vec<Modality>) -> ModelInfo {
        ModelInfo {
            id: "m".into(),
            display_name: "M".into(),
            created_at: None,
            context_window: None,
            max_output: None,
            pricing: None,
            capabilities: ModelCapabilities {
                output,
                ..Default::default()
            },
        }
    }

    #[test]
    fn a_model_goes_to_the_endpoint_that_can_serve_it() {
        assert_eq!(
            route(Some(&info(vec![Modality::Video]))),
            Some(MediaRoute::Video)
        );
        assert_eq!(
            route(Some(&info(vec![Modality::Speech]))),
            Some(MediaRoute::Speech)
        );
        assert_eq!(
            route(Some(&info(vec![Modality::Image]))),
            Some(MediaRoute::Image)
        );
        // A model that answers with a picture *and* text is a chat model, and stays one.
        assert_eq!(
            route(Some(&info(vec![Modality::Text, Modality::Image]))),
            None
        );
        assert_eq!(
            route(Some(&info(vec![Modality::Text, Modality::Audio]))),
            None
        );
        assert_eq!(route(Some(&info(vec![Modality::Text]))), None);
        // A model nobody described is a chat model: that is what it was before the list grew.
        assert_eq!(route(None), None);
    }

    #[test]
    fn the_prompt_is_the_last_thing_the_user_said() {
        let mut req = ChatRequest::new(
            "m",
            "sys",
            vec![
                Message::user_text("a cat"),
                Message::user_text("a cat on a skateboard"),
            ],
        );
        assert_eq!(prompt(&req), "a cat on a skateboard");
        req.messages.clear();
        assert_eq!(prompt(&req), "");
    }
}
