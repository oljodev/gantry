//! Interactions API SSE events → [`StreamEvent`]s. Block indices are step indices; a step
//! without one gets the next free index. Unknown event and delta types are ignored, as Google
//! documents new ones will appear.

use std::collections::BTreeMap;

use futures_util::Stream;
use gantry_core::{CallId, ContentPart, ProviderErrorKind, StopReason, Usage};
use serde_json::Value;

use crate::{error::ProviderError, provider::StreamEvent, pump::StreamParser, sse::SseEvent};

#[derive(Debug)]
enum Step {
    Text,
    Thought,
    FunctionCall {
        id: CallId,
        name: String,
        args: String,
        whole: Option<Value>,
        signature: Option<String>,
    },
    Other,
}

#[derive(Debug, Default)]
pub struct EventParser {
    started: bool,
    next_index: u32,
    current: Option<u32>,
    steps: BTreeMap<u32, Step>,
    saw_call: bool,
    usage: Option<Usage>,
    stop: Option<StopReason>,
    ended: bool,
}

impl EventParser {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn index_of(&mut self, ev: &Value, fresh: bool) -> u32 {
        if let Some(i) = ev
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|i| u32::try_from(i).ok())
        {
            self.next_index = self.next_index.max(i + 1);
            self.current = Some(i);
            return i;
        }
        if fresh || self.current.is_none() {
            let i = self.next_index;
            self.next_index += 1;
            self.current = Some(i);
            return i;
        }
        self.current.unwrap_or(0)
    }

    /// One SSE event → events. `event_name` is the SSE `event:` line, used when the payload
    /// carries no `event_type`.
    pub fn parse(
        &mut self,
        event_name: Option<&str>,
        data: &str,
    ) -> Result<Vec<StreamEvent>, ProviderError> {
        if self.ended || data.trim().is_empty() || data.trim() == "[DONE]" {
            return Ok(Vec::new());
        }
        let ev: Value = serde_json::from_str(data)
            .map_err(|e| ProviderError::interrupted(format!("malformed stream event: {e}")))?;
        let kind = ev
            .get("event_type")
            .or_else(|| ev.get("type"))
            .and_then(Value::as_str)
            .or(event_name)
            .unwrap_or("");
        let mut out = Vec::new();
        match kind {
            "interaction.created" => {
                self.started = true;
                out.push(StreamEvent::MessageStart {
                    provider_message_id: ev
                        .get("interaction")
                        .and_then(|i| i.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                });
            }
            "step.start" => {
                let index = self.index_of(&ev, true);
                let step = ev
                    .get("step")
                    .or_else(|| ev.get("content"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let state = match step.get("type").and_then(Value::as_str).unwrap_or("") {
                    "function_call" => {
                        self.saw_call = true;
                        let id = step
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map_or_else(CallId::new, |s| CallId(s.to_owned()));
                        let name = step
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        out.push(StreamEvent::ToolCallStart {
                            index,
                            id: id.clone(),
                            name: name.clone(),
                        });
                        Step::FunctionCall {
                            id,
                            name,
                            args: String::new(),
                            whole: step.get("arguments").filter(|a| a.is_object()).cloned(),
                            signature: step
                                .get("thought_signature")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        }
                    }
                    "thought" => Step::Thought,
                    "model_output" | "text" => Step::Text,
                    _ => Step::Other,
                };
                self.steps.insert(index, state);
            }
            "step.delta" => {
                let index = self.index_of(&ev, false);
                let delta = ev.get("delta").cloned().unwrap_or(Value::Null);
                match delta.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text" => {
                        if let Some(text) = delta.get("text").and_then(Value::as_str)
                            && !text.is_empty()
                        {
                            self.steps.entry(index).or_insert(Step::Text);
                            out.push(StreamEvent::TextDelta {
                                index,
                                text: text.to_owned(),
                            });
                        }
                    }
                    "thought_summary" => {
                        let text = delta
                            .get("content")
                            .and_then(|c| c.get("text"))
                            .or_else(|| delta.get("text"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if !text.is_empty() {
                            self.steps.entry(index).or_insert(Step::Thought);
                            out.push(StreamEvent::ThinkingDelta {
                                index,
                                text: text.to_owned(),
                            });
                        }
                    }
                    "arguments_delta" => {
                        let fragment = delta.get("arguments").and_then(Value::as_str).unwrap_or("");
                        if let Some(Step::FunctionCall { args, .. }) = self.steps.get_mut(&index)
                            && !fragment.is_empty()
                        {
                            args.push_str(fragment);
                            out.push(StreamEvent::ToolCallArgsDelta {
                                index,
                                json_fragment: fragment.to_owned(),
                            });
                        }
                    }
                    "thought_signature" => {
                        let sig = delta
                            .get("signature")
                            .or_else(|| delta.get("thought_signature"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        match self.steps.get_mut(&index) {
                            Some(Step::FunctionCall { signature, .. }) => {
                                *signature = Some(sig.to_owned());
                            }
                            _ => {
                                self.steps.entry(index).or_insert(Step::Thought);
                                out.push(StreamEvent::ThinkingSignature {
                                    index,
                                    signature: sig.to_owned(),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            "step.stop" => {
                let index = self.index_of(&ev, false);
                if let Some(Step::FunctionCall {
                    id,
                    name,
                    args,
                    whole,
                    signature,
                }) = self.steps.remove(&index)
                {
                    let final_step = ev.get("step").cloned().unwrap_or(Value::Null);
                    let args = final_step
                        .get("arguments")
                        .filter(|a| a.is_object())
                        .cloned()
                        .or(whole)
                        .unwrap_or_else(|| parse_args(&args));
                    let signature = final_step
                        .get("thought_signature")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or(signature);
                    out.push(StreamEvent::ToolCallEnd {
                        index,
                        args: args.clone(),
                    });
                    if signature.is_some() {
                        out.push(StreamEvent::ProviderBlock {
                            index,
                            part: ContentPart::ToolCall {
                                id,
                                name,
                                args,
                                signature,
                            },
                        });
                    }
                }
                self.current = None;
            }
            "interaction.completed" => {
                let interaction = ev.get("interaction").cloned().unwrap_or(Value::Null);
                self.usage = interaction.get("usage").map(usage);
                let status = interaction
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("completed");
                self.stop = Some(match status {
                    "incomplete" | "max_tokens" => StopReason::MaxTokens,
                    _ if self.saw_call => StopReason::ToolUse,
                    _ => StopReason::EndTurn,
                });
                out.extend(self.finish());
            }
            "error" => {
                self.ended = true;
                let err = ev.get("error").cloned().unwrap_or(Value::Null);
                let message = err
                    .get("message")
                    .or_else(|| ev.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("provider error");
                let kind = match err.get("status").and_then(Value::as_str).unwrap_or("") {
                    "RESOURCE_EXHAUSTED" => ProviderErrorKind::RateLimited,
                    "UNAVAILABLE" => ProviderErrorKind::Overloaded,
                    "UNAUTHENTICATED" | "PERMISSION_DENIED" => ProviderErrorKind::Auth,
                    "INVALID_ARGUMENT" => ProviderErrorKind::InvalidRequest,
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
        for (index, step) in std::mem::take(&mut self.steps) {
            if let Step::FunctionCall { args, whole, .. } = step {
                out.push(StreamEvent::ToolCallEnd {
                    index,
                    args: whole.unwrap_or_else(|| parse_args(&args)),
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
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    Usage {
        input: n("total_input_tokens"),
        output: n("total_output_tokens"),
        cache_read: n("total_cached_tokens"),
        cache_write: 0,
        reasoning: n("total_thought_tokens"),
        ..Usage::default()
    }
}

impl StreamParser for EventParser {
    fn parse(&mut self, event: &SseEvent) -> Result<Vec<StreamEvent>, ProviderError> {
        EventParser::parse(self, event.event.as_deref(), &event.data)
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
