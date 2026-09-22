//! Responses API SSE events → [`StreamEvent`]s. Block indices are the response's output item
//! indices; a reasoning item becomes one `Thinking` part carrying its id and encrypted content
//! so the next round can replay it.

use std::collections::BTreeMap;

use futures_util::Stream;
use gantry_core::{CallId, ContentPart, ProviderErrorKind, ProviderKind, StopReason, Usage};
use serde_json::Value;

use crate::{error::ProviderError, provider::StreamEvent, pump::StreamParser, sse::SseEvent};

#[derive(Debug)]
enum Item {
    FunctionCall { args: String, ended: bool },
    Reasoning { summary: String },
    Other,
}

#[derive(Debug, Default)]
pub struct EventParser {
    started: bool,
    items: BTreeMap<u32, Item>,
    saw_call: bool,
    refused: bool,
    usage: Option<Usage>,
    stop: Option<StopReason>,
    ended: bool,
}

impl EventParser {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(&mut self, data: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        if self.ended || data.trim().is_empty() || data.trim() == "[DONE]" {
            return Ok(Vec::new());
        }
        let ev: Value = serde_json::from_str(data)
            .map_err(|e| ProviderError::interrupted(format!("malformed stream event: {e}")))?;
        let kind = ev.get("type").and_then(Value::as_str).unwrap_or("");
        let index = ev
            .get("output_index")
            .and_then(Value::as_u64)
            .and_then(|i| u32::try_from(i).ok())
            .unwrap_or(0);
        let delta = ev.get("delta").and_then(Value::as_str).unwrap_or("");
        let mut out = Vec::new();
        match kind {
            "response.created" => {
                self.started = true;
                out.push(StreamEvent::MessageStart {
                    provider_message_id: ev
                        .get("response")
                        .and_then(|r| r.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                });
            }
            "response.output_item.added" => {
                let item = ev.get("item").cloned().unwrap_or(Value::Null);
                let state = match item.get("type").and_then(Value::as_str).unwrap_or("") {
                    "function_call" => {
                        self.saw_call = true;
                        let id = item
                            .get("call_id")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map_or_else(CallId::new, |s| CallId(s.to_owned()));
                        out.push(StreamEvent::ToolCallStart {
                            index,
                            id,
                            name: item
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                        });
                        Item::FunctionCall {
                            args: String::new(),
                            ended: false,
                        }
                    }
                    "reasoning" => Item::Reasoning {
                        summary: String::new(),
                    },
                    _ => Item::Other,
                };
                self.items.insert(index, state);
            }
            "response.output_text.delta" => {
                if !delta.is_empty() {
                    out.push(StreamEvent::TextDelta {
                        index,
                        text: delta.to_owned(),
                    });
                }
            }
            "response.refusal.delta" => {
                self.refused = true;
                if !delta.is_empty() {
                    out.push(StreamEvent::TextDelta {
                        index,
                        text: delta.to_owned(),
                    });
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                if !delta.is_empty() {
                    if let Some(Item::Reasoning { summary }) = self.items.get_mut(&index) {
                        summary.push_str(delta);
                    }
                    out.push(StreamEvent::ThinkingDelta {
                        index,
                        text: delta.to_owned(),
                    });
                }
            }
            "response.reasoning_summary_part.added" => {
                // Summary parts are paragraphs; a blank line separates each from the previous.
                let part_index = ev.get("summary_index").and_then(Value::as_u64).unwrap_or(0);
                if part_index > 0
                    && let Some(Item::Reasoning { summary }) = self.items.get_mut(&index)
                    && !summary.is_empty()
                {
                    summary.push_str("\n\n");
                    out.push(StreamEvent::ThinkingDelta {
                        index,
                        text: "\n\n".to_owned(),
                    });
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some(Item::FunctionCall { args, .. }) = self.items.get_mut(&index) {
                    args.push_str(delta);
                }
                if !delta.is_empty() {
                    out.push(StreamEvent::ToolCallArgsDelta {
                        index,
                        json_fragment: delta.to_owned(),
                    });
                }
            }
            "response.function_call_arguments.done" => {
                let text = ev
                    .get("arguments")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if let Some(Item::FunctionCall { args, ended }) = self.items.get_mut(&index)
                    && !*ended
                {
                    *ended = true;
                    let raw = text.unwrap_or_else(|| args.clone());
                    out.push(StreamEvent::ToolCallEnd {
                        index,
                        args: parse_args(&raw),
                    });
                }
            }
            "response.output_item.done" => {
                let item = ev.get("item").cloned().unwrap_or(Value::Null);
                match item.get("type").and_then(Value::as_str).unwrap_or("") {
                    "function_call" => {
                        if let Some(Item::FunctionCall { args, ended }) = self.items.get_mut(&index)
                            && !*ended
                        {
                            *ended = true;
                            let raw = item
                                .get("arguments")
                                .and_then(Value::as_str)
                                .map_or_else(|| args.clone(), str::to_owned);
                            out.push(StreamEvent::ToolCallEnd {
                                index,
                                args: parse_args(&raw),
                            });
                        }
                    }
                    "reasoning" => {
                        let summary = item
                            .get("summary")
                            .and_then(Value::as_array)
                            .map(|parts| {
                                parts
                                    .iter()
                                    .filter_map(|p| p.get("text").and_then(Value::as_str))
                                    .collect::<Vec<_>>()
                                    .join("\n\n")
                            })
                            .filter(|s| !s.is_empty())
                            .or_else(|| match self.items.get(&index) {
                                Some(Item::Reasoning { summary }) => {
                                    Some(summary.trim_end().to_owned())
                                }
                                _ => None,
                            })
                            .unwrap_or_default();
                        let encrypted = item
                            .get("encrypted_content")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                        let id = item.get("id").and_then(Value::as_str).map(str::to_owned);
                        if encrypted.is_some() || !summary.is_empty() {
                            out.push(StreamEvent::ProviderBlock {
                                index,
                                part: ContentPart::Thinking {
                                    text: summary,
                                    signature: encrypted,
                                    provider: ProviderKind::OpenAiResponses,
                                    item_id: id,
                                },
                            });
                        }
                    }
                    "web_search_call" | "web_fetch_call" | "file_search_call" => {
                        out.push(StreamEvent::ProviderBlock {
                            index,
                            part: ContentPart::ProviderOpaque {
                                provider: ProviderKind::OpenAiResponses,
                                block_kind: item
                                    .get("type")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_owned(),
                                json: item,
                            },
                        });
                    }
                    _ => {}
                }
            }
            "response.completed" | "response.incomplete" => {
                let response = ev.get("response").cloned().unwrap_or(Value::Null);
                self.usage = response.get("usage").map(usage);
                self.stop = Some(if kind == "response.incomplete" {
                    match response
                        .get("incomplete_details")
                        .and_then(|d| d.get("reason"))
                        .and_then(Value::as_str)
                    {
                        Some("content_filter") => StopReason::ContentFilter,
                        _ => StopReason::MaxTokens,
                    }
                } else if self.saw_call {
                    StopReason::ToolUse
                } else if self.refused {
                    StopReason::Refusal { category: None }
                } else {
                    StopReason::EndTurn
                });
                out.extend(self.finish());
            }
            "response.failed" => {
                self.ended = true;
                let message = ev
                    .get("response")
                    .and_then(|r| r.get("error"))
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("the response failed");
                return Err(ProviderError::new(ProviderErrorKind::Unknown, message));
            }
            "error" => {
                self.ended = true;
                let message = ev
                    .get("message")
                    .or_else(|| ev.get("error").and_then(|e| e.get("message")))
                    .and_then(Value::as_str)
                    .unwrap_or("provider error");
                let code = ev
                    .get("code")
                    .or_else(|| ev.get("error").and_then(|e| e.get("code")))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let kind = match code {
                    "rate_limit_exceeded" => ProviderErrorKind::RateLimited,
                    "server_error" => ProviderErrorKind::Overloaded,
                    "invalid_api_key" => ProviderErrorKind::Auth,
                    "context_length_exceeded" => ProviderErrorKind::ContextTooLong,
                    _ => ProviderErrorKind::Unknown,
                };
                return Err(ProviderError::new(kind, message));
            }
            _ => {}
        }
        Ok(out)
    }

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
        for (index, item) in std::mem::take(&mut self.items) {
            if let Item::FunctionCall { args, ended: false } = item {
                out.push(StreamEvent::ToolCallEnd {
                    index,
                    args: parse_args(&args),
                });
            }
        }
        if let Some(u) = self.usage.take() {
            out.push(StreamEvent::Usage(u));
        }
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

fn parse_args(raw: &str) -> Value {
    if raw.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(raw).unwrap_or(Value::Null)
    }
}

fn usage(u: &Value) -> Usage {
    let n = |path: &[&str]| -> u64 {
        let mut v = u;
        for p in path {
            v = match v.get(p) {
                Some(x) => x,
                None => return 0,
            };
        }
        v.as_u64().unwrap_or(0)
    };
    Usage {
        input: n(&["input_tokens"]),
        output: n(&["output_tokens"]),
        cache_read: n(&["input_tokens_details", "cached_tokens"]),
        cache_write: 0,
        reasoning: n(&["output_tokens_details", "reasoning_tokens"]),
        ..Usage::default()
    }
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
