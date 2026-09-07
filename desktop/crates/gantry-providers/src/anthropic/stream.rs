//! Messages API SSE events → [`StreamEvent`]s. Block indices are Anthropic's own content block
//! indices; server-tool blocks are collected whole and handed over as opaque parts.

use std::collections::BTreeMap;

use futures_util::Stream;
use gantry_core::{CallId, ContentPart, ProviderErrorKind, ProviderKind, StopReason, Usage};
use serde::Deserialize;
use serde_json::Value;

use crate::{error::ProviderError, provider::StreamEvent, pump::StreamParser, sse::SseEvent};

#[derive(Debug, Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    message: Option<MessageStart>,
    index: Option<u32>,
    content_block: Option<Value>,
    delta: Option<Value>,
    usage: Option<WireUsage>,
    error: Option<WireError>,
}

#[derive(Debug, Deserialize)]
struct MessageStart {
    id: Option<String>,
    usage: Option<WireUsage>,
}

#[derive(Debug, Default, Deserialize)]
struct WireUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct WireError {
    #[serde(rename = "type")]
    kind: Option<String>,
    message: Option<String>,
}

#[derive(Debug)]
enum Block {
    Text,
    Thinking,
    ToolUse {
        args: String,
    },
    /// A server tool block whose `input` streams in; replayed whole.
    ServerToolUse {
        block: Value,
        args: String,
    },
    Other,
}

/// Parses one stream's events in order.
#[derive(Debug, Default)]
pub struct EventParser {
    started: bool,
    blocks: BTreeMap<u32, Block>,
    usage: Usage,
    stop: Option<StopReason>,
    ended: bool,
}

impl EventParser {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One SSE payload → events. `message_stop` closes the message.
    pub fn parse(&mut self, data: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        if self.ended || data.trim().is_empty() {
            return Ok(Vec::new());
        }
        let ev: Event = serde_json::from_str(data)
            .map_err(|e| ProviderError::interrupted(format!("malformed stream event: {e}")))?;
        let mut out = Vec::new();
        match ev.kind.as_str() {
            "message_start" => {
                self.started = true;
                let m = ev.message.unwrap_or(MessageStart {
                    id: None,
                    usage: None,
                });
                if let Some(u) = m.usage {
                    self.usage.input = u.input_tokens.unwrap_or(0);
                    self.usage.cache_read = u.cache_read_input_tokens.unwrap_or(0);
                    self.usage.cache_write = u.cache_creation_input_tokens.unwrap_or(0);
                    self.usage.output = u.output_tokens.unwrap_or(0);
                }
                out.push(StreamEvent::MessageStart {
                    provider_message_id: m.id,
                });
            }
            "content_block_start" => {
                let index = ev.index.unwrap_or(0);
                let block = ev.content_block.unwrap_or(Value::Null);
                let kind = block
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let state = match kind.as_str() {
                    "text" => Block::Text,
                    "thinking" => Block::Thinking,
                    "tool_use" => {
                        let id = block
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map_or_else(CallId::new, |s| CallId(s.to_owned()));
                        out.push(StreamEvent::ToolCallStart {
                            index,
                            id,
                            name: block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                        });
                        Block::ToolUse {
                            args: String::new(),
                        }
                    }
                    "server_tool_use" => Block::ServerToolUse {
                        block,
                        args: String::new(),
                    },
                    "redacted_thinking" | "web_search_tool_result" | "web_fetch_tool_result" => {
                        out.push(StreamEvent::ProviderBlock {
                            index,
                            part: opaque(&kind, block),
                        });
                        Block::Other
                    }
                    _ => Block::Other,
                };
                self.blocks.insert(index, state);
            }
            "content_block_delta" => {
                let index = ev.index.unwrap_or(0);
                let delta = ev.delta.unwrap_or(Value::Null);
                let kind = delta.get("type").and_then(Value::as_str).unwrap_or("");
                match kind {
                    "text_delta" => {
                        if let Some(text) = delta.get("text").and_then(Value::as_str)
                            && !text.is_empty()
                        {
                            self.blocks.entry(index).or_insert(Block::Text);
                            out.push(StreamEvent::TextDelta {
                                index,
                                text: text.to_owned(),
                            });
                        }
                    }
                    "thinking_delta" => {
                        if let Some(text) = delta.get("thinking").and_then(Value::as_str)
                            && !text.is_empty()
                        {
                            self.blocks.entry(index).or_insert(Block::Thinking);
                            out.push(StreamEvent::ThinkingDelta {
                                index,
                                text: text.to_owned(),
                            });
                        }
                    }
                    "signature_delta" => {
                        if let Some(sig) = delta.get("signature").and_then(Value::as_str) {
                            out.push(StreamEvent::ThinkingSignature {
                                index,
                                signature: sig.to_owned(),
                            });
                        }
                    }
                    "input_json_delta" => {
                        let fragment = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        match self.blocks.get_mut(&index) {
                            Some(Block::ToolUse { args }) => {
                                args.push_str(fragment);
                                if !fragment.is_empty() {
                                    out.push(StreamEvent::ToolCallArgsDelta {
                                        index,
                                        json_fragment: fragment.to_owned(),
                                    });
                                }
                            }
                            Some(Block::ServerToolUse { args, .. }) => args.push_str(fragment),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let index = ev.index.unwrap_or(0);
                match self.blocks.remove(&index) {
                    Some(Block::ToolUse { args }) => {
                        let args = if args.trim().is_empty() {
                            serde_json::json!({})
                        } else {
                            serde_json::from_str(&args).unwrap_or(Value::Null)
                        };
                        out.push(StreamEvent::ToolCallEnd { index, args });
                    }
                    Some(Block::ServerToolUse { mut block, args }) => {
                        if !args.trim().is_empty()
                            && let Ok(input) = serde_json::from_str::<Value>(&args)
                        {
                            block["input"] = input;
                        }
                        out.push(StreamEvent::ProviderBlock {
                            index,
                            part: opaque("server_tool_use", block),
                        });
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(u) = ev.usage {
                    if let Some(o) = u.output_tokens {
                        self.usage.output = o;
                    }
                    if let Some(i) = u.input_tokens {
                        self.usage.input = i;
                    }
                    if let Some(c) = u.cache_read_input_tokens {
                        self.usage.cache_read = c;
                    }
                    if let Some(c) = u.cache_creation_input_tokens {
                        self.usage.cache_write = c;
                    }
                }
                if let Some(delta) = ev.delta
                    && let Some(reason) = delta.get("stop_reason").and_then(Value::as_str)
                {
                    let category = delta
                        .get("stop_details")
                        .and_then(|d| d.get("category"))
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    self.stop = Some(stop_reason(reason, category));
                }
            }
            "message_stop" => {
                out.extend(self.finish());
            }
            "error" => {
                self.ended = true;
                return Err(wire_error(ev.error));
            }
            _ => {}
        }
        Ok(out)
    }

    /// Closes the message: open tool calls end, usage and the stop reason are emitted.
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
        for (index, block) in std::mem::take(&mut self.blocks) {
            if let Block::ToolUse { args } = block {
                let args = serde_json::from_str(&args).unwrap_or(serde_json::json!({}));
                out.push(StreamEvent::ToolCallEnd { index, args });
            }
        }
        out.push(StreamEvent::Usage(self.usage));
        out.push(StreamEvent::MessageEnd {
            stop_reason: self.stop.take().unwrap_or(StopReason::EndTurn),
        });
        out
    }

    #[must_use]
    pub fn ended(&self) -> bool {
        self.ended
    }
}

fn opaque(kind: &str, block: Value) -> ContentPart {
    ContentPart::ProviderOpaque {
        provider: ProviderKind::Anthropic,
        block_kind: kind.to_owned(),
        json: block,
    }
}

fn stop_reason(reason: &str, category: Option<String>) -> StopReason {
    match reason {
        "end_turn" | "stop_sequence" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "refusal" => StopReason::Refusal { category },
        "pause_turn" => StopReason::PauseTurn,
        other => StopReason::Other {
            reason: other.to_owned(),
        },
    }
}

fn wire_error(err: Option<WireError>) -> ProviderError {
    let (kind, message) = err
        .map(|e| (e.kind.unwrap_or_default(), e.message))
        .unwrap_or_default();
    let message = message.unwrap_or_else(|| "provider error".to_owned());
    let kind = match kind.as_str() {
        "overloaded_error" => ProviderErrorKind::Overloaded,
        "rate_limit_error" => ProviderErrorKind::RateLimited,
        "authentication_error" | "permission_error" => ProviderErrorKind::Auth,
        "invalid_request_error" => ProviderErrorKind::InvalidRequest,
        "not_found_error" => ProviderErrorKind::NotFound,
        _ => ProviderErrorKind::Unknown,
    };
    ProviderError::new(kind, message)
}

impl StreamParser for EventParser {
    fn parse(&mut self, event: &SseEvent) -> Result<Vec<StreamEvent>, ProviderError> {
        EventParser::parse(self, &event.data)
    }

    fn finish(&mut self) -> Vec<StreamEvent> {
        EventParser::finish(self)
    }

    fn ended(&self) -> bool {
        self.ended
    }
}

/// Wraps an SSE event stream into a [`ChatStream`](crate::provider::ChatStream).
pub fn into_chat_stream<S>(sse: S) -> crate::provider::ChatStream
where
    S: Stream<Item = Result<SseEvent, ProviderError>> + Send + 'static,
{
    crate::pump::into_chat_stream(sse, EventParser::new())
}
