//! Chat Completions chunks → [`StreamEvent`]s. Block indices are assigned in order of first
//! appearance (reasoning usually precedes text); tool calls keep the provider's own index.

use std::collections::BTreeMap;

use futures_util::{Stream, StreamExt};
use gantry_core::{CallId, ProviderErrorKind, StopReason, Usage};
use serde::Deserialize;

use super::profiles::ToolIdQuirk;
use crate::{error::ProviderError, provider::StreamEvent, sse::SseEvent};

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

/// Wraps an SSE event stream into a [`ChatStream`](crate::provider::ChatStream).
pub fn into_chat_stream<S>(sse: S, quirk: ToolIdQuirk) -> crate::provider::ChatStream
where
    S: Stream<Item = Result<SseEvent, ProviderError>> + Send + 'static,
{
    let stream = futures_util::stream::unfold(
        (
            Box::pin(sse),
            ChunkParser::new(quirk),
            Vec::<StreamEvent>::new(),
            false,
            None::<ProviderError>,
        ),
        |(mut sse, mut parser, mut pending, mut done, mut pending_error)| async move {
            loop {
                if !pending.is_empty() {
                    let ev = pending.remove(0);
                    return Some((Ok(ev), (sse, parser, pending, done, pending_error)));
                }
                if let Some(err) = pending_error.take() {
                    return Some((Err(err), (sse, parser, pending, done, None)));
                }
                if done {
                    return None;
                }
                match sse.next().await {
                    None => {
                        done = true;
                        if !parser.ended() {
                            // The connection closed without `[DONE]`: flush usage and open
                            // tool calls, drop the synthetic end, then report the interruption
                            // so the turn ends as failed with its partial text kept (02 §7).
                            pending.extend(parser.finish());
                            pending.retain(|e| !matches!(e, StreamEvent::MessageEnd { .. }));
                            pending_error =
                                Some(ProviderError::interrupted("the stream ended early"));
                        }
                    }
                    Some(Err(e)) => {
                        done = true;
                        return Some((Err(e), (sse, parser, pending, done, pending_error)));
                    }
                    Some(Ok(ev)) => match parser.parse(&ev.data) {
                        Ok(events) => {
                            pending.extend(events);
                            if parser.ended() {
                                done = true;
                            }
                        }
                        Err(e) => {
                            done = true;
                            return Some((Err(e), (sse, parser, pending, done, pending_error)));
                        }
                    },
                }
            }
        },
    );
    Box::pin(stream)
}
