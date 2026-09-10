//! Chat Completions chunks → [`StreamEvent`]s. Block indices are assigned in order of first
//! appearance (reasoning usually precedes text); tool calls keep the provider's own index.

use std::collections::BTreeMap;

use futures_util::Stream;
use gantry_core::{CallId, ContentPart, MediaSource, ProviderErrorKind, StopReason, Usage};
use serde::Deserialize;

use base64::Engine;

use super::profiles::ToolIdQuirk;
use crate::{error::ProviderError, provider::StreamEvent, pump::StreamParser, sse::SseEvent};

/// What `audio.format` asks for in the request, and therefore what comes back. MP3 frames
/// concatenate cleanly, which a stream delivered in fragments needs, and every webview plays it.
pub const AUDIO_FORMAT: &str = "mp3";
pub const AUDIO_MIME: &str = "audio/mpeg";

#[derive(Debug, Deserialize)]
struct Chunk {
    id: Option<String>,
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<WireUsage>,
    error: Option<WireError>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    #[serde(default)]
    delta: Delta,
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Delta {
    content: Option<String>,
    reasoning: Option<String>,
    #[serde(default)]
    reasoning_details: Vec<ReasoningDetail>,
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
    /// Pictures an image model drew, as data URLs. They arrive whole rather than in deltas.
    #[serde(default)]
    images: Vec<ImageDelta>,
    /// Sound, in pieces: a model that answers aloud sends base64 fragments of one file, with
    /// the words it is saying beside them.
    audio: Option<AudioDelta>,
}

#[derive(Debug, Deserialize)]
struct AudioDelta {
    data: Option<String>,
    transcript: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImageDelta {
    image_url: Option<ImageUrl>,
}

#[derive(Debug, Deserialize)]
struct ImageUrl {
    url: Option<String>,
}

/// `data:image/png;base64,…` → the mime and the bytes. An `http(s)` url is left alone: nothing
/// downloads it here, and a part pointing at a URL the app never fetched would be a lie.
fn data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mime = meta.strip_suffix(";base64")?;
    (!mime.is_empty() && !data.is_empty()).then(|| (mime.to_owned(), data.to_owned()))
}

#[derive(Debug, Deserialize)]
struct ReasoningDetail {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
    summary: Option<String>,
    signature: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    #[serde(default)]
    index: u32,
    id: Option<String>,
    #[serde(default)]
    function: FunctionDelta,
}

#[derive(Debug, Default, Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
    prompt_tokens_details: Option<PromptDetails>,
    completion_tokens_details: Option<CompletionDetails>,
    cost: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct PromptDetails {
    cached_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct CompletionDetails {
    reasoning_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct WireError {
    code: Option<serde_json::Value>,
    message: Option<String>,
    metadata: Option<serde_json::Value>,
}

#[derive(Debug)]
struct ToolState {
    index: u32,
    args: String,
}

/// Parses one stream's chunks in order.
#[derive(Debug)]
pub struct ChunkParser {
    quirk: ToolIdQuirk,
    started: bool,
    next_index: u32,
    thinking_index: Option<u32>,
    text_index: Option<u32>,
    tools: BTreeMap<u32, ToolState>,
    /// The sound so far, as bytes rather than as base64 text: the fragments are separately
    /// encoded, so concatenating the strings would produce padding in the middle of the file.
    audio: Option<(u32, Vec<u8>)>,
    finish: Option<StopReason>,
    usage: Option<Usage>,
    ended: bool,
}

impl ChunkParser {
    #[must_use]
    pub fn new(quirk: ToolIdQuirk) -> Self {
        Self {
            quirk,
            started: false,
            next_index: 0,
            thinking_index: None,
            text_index: None,
            tools: BTreeMap::new(),
            audio: None,
            finish: None,
            usage: None,
            ended: false,
        }
    }

    fn alloc(&mut self) -> u32 {
        let i = self.next_index;
        self.next_index += 1;
        i
    }

    /// One SSE `data` payload → events. `[DONE]` closes the message.
    pub fn parse(&mut self, data: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        if self.ended {
            return Ok(Vec::new());
        }
        if data.trim() == "[DONE]" {
            return Ok(self.finish());
        }
        let chunk: Chunk = serde_json::from_str(data)
            .map_err(|e| ProviderError::interrupted(format!("malformed stream chunk: {e}")))?;
        if let Some(err) = chunk.error {
            self.ended = true;
            return Err(wire_error(err));
        }
        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            out.push(StreamEvent::MessageStart {
                provider_message_id: chunk.id,
            });
        }
        for choice in chunk.choices {
            let d = choice.delta;
            let reasoning_text = match d.reasoning {
                Some(r) if !r.is_empty() => Some(r),
                _ => d
                    .reasoning_details
                    .iter()
                    .filter(|r| {
                        r.kind.as_deref() == Some("reasoning.text")
                            || r.kind.as_deref() == Some("reasoning.summary")
                    })
                    .filter_map(|r| r.text.clone().or_else(|| r.summary.clone()))
                    .filter(|t| !t.is_empty())
                    .reduce(|a, b| a + &b),
            };
            if let Some(text) = reasoning_text {
                let index = match self.thinking_index {
                    Some(i) => i,
                    None => {
                        let i = self.alloc();
                        self.thinking_index = Some(i);
                        i
                    }
                };
                out.push(StreamEvent::ThinkingDelta { index, text });
            }
            for sig in d
                .reasoning_details
                .iter()
                .filter_map(|r| r.signature.clone())
            {
                if let Some(index) = self.thinking_index {
                    out.push(StreamEvent::ThinkingSignature {
                        index,
                        signature: sig,
                    });
                }
            }
            if let Some(text) = d.content
                && !text.is_empty()
            {
                let index = match self.text_index {
                    Some(i) => i,
                    None => {
                        let i = self.alloc();
                        self.text_index = Some(i);
                        i
                    }
                };
                out.push(StreamEvent::TextDelta { index, text });
            }
            for image in &d.images {
                let Some((mime, data)) = image
                    .image_url
                    .as_ref()
                    .and_then(|u| u.url.as_deref())
                    .and_then(data_url)
                else {
                    continue;
                };
                let index = self.alloc();
                out.push(StreamEvent::ProviderBlock {
                    index,
                    part: ContentPart::Image {
                        source: MediaSource::Base64 { data },
                        mime,
                    },
                });
            }
            if let Some(audio) = d.audio {
                // The words are streamed as text, which is what makes them readable while the
                // sound is still arriving and searchable afterwards.
                if let Some(text) = audio.transcript.filter(|t| !t.is_empty()) {
                    let index = match self.text_index {
                        Some(i) => i,
                        None => {
                            let i = self.alloc();
                            self.text_index = Some(i);
                            i
                        }
                    };
                    out.push(StreamEvent::TextDelta { index, text });
                }
                if let Some(fragment) = audio.data.filter(|d| !d.is_empty())
                    && let Ok(bytes) =
                        base64::engine::general_purpose::STANDARD.decode(fragment.as_bytes())
                {
                    let (_, buffer) = self
                        .audio
                        .get_or_insert_with(|| (self.next_index, Vec::new()));
                    buffer.extend_from_slice(&bytes);
                }
            }
            for tc in d.tool_calls {
                let state = if let Some(s) = self.tools.get_mut(&tc.index) {
                    s
                } else {
                    let index = self.alloc();
                    let id = match tc.id.as_deref() {
                        Some(id) if !id.is_empty() => CallId(id.to_owned()),
                        _ if self.quirk == ToolIdQuirk::SynthesizeIfEmpty => CallId::new(),
                        _ => CallId(String::new()),
                    };
                    out.push(StreamEvent::ToolCallStart {
                        index,
                        id,
                        name: tc.function.name.clone().unwrap_or_default(),
                    });
                    self.tools.entry(tc.index).or_insert(ToolState {
                        index,
                        args: String::new(),
                    })
                };
                if let Some(fragment) = tc.function.arguments
                    && !fragment.is_empty()
                {
                    state.args.push_str(&fragment);
                    out.push(StreamEvent::ToolCallArgsDelta {
                        index: state.index,
                        json_fragment: fragment,
                    });
                }
            }
            if let Some(reason) = choice.finish_reason {
                self.finish = Some(stop_reason(&reason));
            }
        }
        if let Some(u) = chunk.usage {
            self.usage = Some(Usage {
                input: u.prompt_tokens,
                output: u.completion_tokens,
                cache_read: u
                    .prompt_tokens_details
                    .and_then(|d| d.cached_tokens)
                    .unwrap_or(0),
                cache_write: 0,
                reasoning: u
                    .completion_tokens_details
                    .and_then(|d| d.reasoning_tokens)
                    .unwrap_or(0),
                cost_usd: u.cost,
            });
        }
        Ok(out)
    }

    /// Closes the message: pending tool calls end, usage and the stop reason are emitted.
    pub fn finish(&mut self) -> Vec<StreamEvent> {
        if self.ended {
            return Vec::new();
        }
        self.ended = true;
        let mut out = Vec::new();
        if !self.started {
            out.push(StreamEvent::MessageStart {
                provider_message_id: None,
            });
        }
        // The sound is one file, and it is only a file once the last fragment has arrived.
        if let Some((index, bytes)) = self.audio.take()
            && !bytes.is_empty()
        {
            out.push(StreamEvent::ProviderBlock {
                index,
                part: ContentPart::Audio {
                    source: MediaSource::Base64 {
                        data: base64::engine::general_purpose::STANDARD.encode(&bytes),
                    },
                    mime: AUDIO_MIME.to_owned(),
                },
            });
        }
        for (_, tool) in std::mem::take(&mut self.tools) {
            let args = serde_json::from_str(&tool.args).unwrap_or(serde_json::Value::Null);
            out.push(StreamEvent::ToolCallEnd {
                index: tool.index,
                args,
            });
        }
        if let Some(u) = self.usage.take() {
            out.push(StreamEvent::Usage(u));
        }
        let stop_reason = self.finish.take().unwrap_or(StopReason::EndTurn);
        out.push(StreamEvent::MessageEnd { stop_reason });
        out
    }

    /// Whether the stream was closed cleanly by `[DONE]` or an error.
    #[must_use]
    pub fn ended(&self) -> bool {
        self.ended
    }
}

fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "stop" | "end_turn" => StopReason::EndTurn,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        "content_filter" => StopReason::ContentFilter,
        other => StopReason::Other {
            reason: other.to_owned(),
        },
    }
}

fn wire_error(err: WireError) -> ProviderError {
    let status = err
        .code
        .as_ref()
        .and_then(|c| c.as_u64())
        .and_then(|c| u16::try_from(c).ok());
    let mut message = err.message.unwrap_or_else(|| "provider error".to_owned());
    if let Some(p) = err
        .metadata
        .as_ref()
        .and_then(|m| m.get("provider_name"))
        .and_then(|p| p.as_str())
    {
        message = format!("{message} ({p})");
    }
    match status {
        Some(s) if s >= 400 => ProviderError::from_status(
            s,
            &format!(
                "{{\"error\":{{\"message\":{}}}}}",
                serde_json::Value::String(message)
            ),
            None,
        ),
        _ => ProviderError::new(ProviderErrorKind::Overloaded, message),
    }
}

impl StreamParser for ChunkParser {
    fn parse(&mut self, event: &SseEvent) -> Result<Vec<StreamEvent>, ProviderError> {
        ChunkParser::parse(self, &event.data)
    }

    fn finish(&mut self) -> Vec<StreamEvent> {
        ChunkParser::finish(self)
    }

    fn ended(&self) -> bool {
        self.ended
    }
}

/// Wraps an SSE event stream into a [`ChatStream`](crate::provider::ChatStream).
pub fn into_chat_stream<S>(sse: S, quirk: ToolIdQuirk) -> crate::provider::ChatStream
where
    S: Stream<Item = Result<SseEvent, ProviderError>> + Send + 'static,
{
    crate::pump::into_chat_stream(sse, ChunkParser::new(quirk))
}
