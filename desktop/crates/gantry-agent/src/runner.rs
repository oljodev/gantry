//! One turn: build the request, stream, convert provider events to agent events, keep the
//! assistant message, record the outcome (docs/plan/01 §3 steps 2, 3 and 7).

use std::{collections::BTreeMap, sync::Arc, time::Instant};

use futures_util::StreamExt;
use gantry_core::{
    AgentEventKind, ContentPart, Message, MessageId, ProviderErrorKind, ProviderKind, Role,
    StopReason, TurnStatus, Usage, now_ms,
};
use gantry_providers::{ChatRequest, Provider, ProviderError, StreamEvent};

use crate::{
    chats::{ChatBook, TurnInput, TurnOutcome},
    turn_manager::ActiveTurn,
};

enum End {
    Completed(StopReason),
    Cancelled,
    Failed(ProviderError),
}

pub async fn run_turn(
    input: TurnInput,
    provider: Option<Arc<dyn Provider>>,
    max_output_tokens: u32,
    active: Arc<ActiveTurn>,
    chats: Arc<ChatBook>,
) {
    let started = Instant::now();
    let batcher = active.batcher.clone();
    batcher.push(AgentEventKind::TurnStarted {
        chat_id: input.chat_id,
        mode: input.mode,
        guard: input.guard,
        model: input.model.clone(),
    });

    let message_id = MessageId::new();
    {
        let mut s = active.state.lock().unwrap_or_else(|e| e.into_inner());
        s.message_id = Some(message_id);
    }
    batcher.push(AgentEventKind::MessageStarted {
        message_id,
        role: Role::Assistant,
    });

    let mut parts: BTreeMap<u32, ContentPart> = BTreeMap::new();
    let mut usage: Option<Usage> = None;
    let provider_kind = provider.as_ref().map(|p| p.kind());

    let end = match provider {
        None => End::Failed(ProviderError::new(
            ProviderErrorKind::NotFound,
            format!("provider {} is not configured", input.model.provider),
        )),
        Some(provider) => {
            let mut req = ChatRequest::new(
                input.model.model.clone(),
                input.system.clone(),
                input.messages.clone(),
            );
            req.max_output_tokens = max_output_tokens;
            req.reasoning = input.effort;
            req.metadata.chat_id = Some(input.chat_id);
            req.metadata.turn_id = Some(input.turn_id);
            let cancel = active.cancel.clone();
            let opened = tokio::select! {
                _ = cancel.cancelled() => Err(None),
                r = provider.stream(req) => r.map_err(Some),
            };
            match opened {
                Err(None) => End::Cancelled,
                Err(Some(err)) => End::Failed(err),
                Ok(mut stream) => {
                    let mut end = None;
                    while end.is_none() {
                        let next = tokio::select! {
                            _ = cancel.cancelled() => { end = Some(End::Cancelled); break; }
                            n = stream.next() => n,
                        };
                        match next {
                            None => {
                                end = Some(End::Failed(ProviderError::interrupted(
                                    "the stream ended without a stop reason",
                                )))
                            }
                            Some(Err(err)) => end = Some(End::Failed(err)),
                            Some(Ok(ev)) => {
                                apply(
                                    ev,
                                    &mut parts,
                                    &mut usage,
                                    &active,
                                    message_id,
                                    provider.kind(),
                                    &batcher,
                                    &mut end,
                                );
                            }
                        }
                    }
                    end.unwrap_or(End::Cancelled)
                }
            }
        }
    };

    let assistant = if parts.is_empty() {
        None
    } else {
        Some(Message {
            id: message_id,
            role: Role::Assistant,
            parts: parts.values().cloned().collect(),
            origin: provider_kind,
            created_at: now_ms(),
        })
    };
    let (status, stop_reason, error) = match &end {
        End::Completed(reason) => (TurnStatus::Completed, Some(reason.clone()), None),
        End::Cancelled => (TurnStatus::Cancelled, Some(StopReason::Cancelled), None),
        End::Failed(err) => (TurnStatus::Failed, None, Some(err.message.clone())),
    };

    // Final parts are authoritative for every consumer.
    for (block, part) in &parts {
        batcher.push(AgentEventKind::BlockDone {
            message_id,
            block: *block,
            part: part.clone(),
        });
    }
    match &end {
        End::Failed(err) => {
            batcher.push(AgentEventKind::Error {
                code: format!("{:?}", err.kind).to_ascii_lowercase(),
                message: err.message.clone(),
                retryable: err.kind.is_retryable(),
            });
        }
        End::Completed(reason) => {
            batcher.push(AgentEventKind::MessageCompleted {
                message_id,
                stop_reason: reason.clone(),
                usage,
            });
        }
        End::Cancelled => {
            batcher.push(AgentEventKind::MessageCompleted {
                message_id,
                stop_reason: StopReason::Cancelled,
                usage,
            });
        }
    }
    {
        let mut s = active.state.lock().unwrap_or_else(|e| e.into_inner());
        s.status = status;
        s.usage = usage;
    }
    chats.finish_turn(
        input.chat_id,
        input.turn_id,
        TurnOutcome {
            status,
            assistant,
            usage,
            stop_reason,
            error,
        },
    );
    batcher.push(AgentEventKind::TurnCompleted {
        status,
        usage,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    });
    batcher.close();
}

#[allow(clippy::too_many_arguments)]
fn apply(
    ev: StreamEvent,
    parts: &mut BTreeMap<u32, ContentPart>,
    usage: &mut Option<Usage>,
    active: &ActiveTurn,
    message_id: MessageId,
    provider: ProviderKind,
    batcher: &crate::events::Batcher,
    end: &mut Option<End>,
) {
    match ev {
        StreamEvent::MessageStart { .. } => {}
        StreamEvent::TextDelta { index, text } => {
            if let ContentPart::Text { text: t } =
                parts.entry(index).or_insert_with(|| ContentPart::Text {
                    text: String::new(),
                })
            {
                t.push_str(&text);
            }
            sync_state(active, parts);
            batcher.push(AgentEventKind::TextDelta {
                message_id,
                block: index,
                text,
            });
        }
        StreamEvent::ThinkingDelta { index, text } => {
            if let ContentPart::Thinking { text: t, .. } =
                parts.entry(index).or_insert_with(|| ContentPart::Thinking {
                    text: String::new(),
                    signature: None,
                    provider,
                })
            {
                t.push_str(&text);
            }
            sync_state(active, parts);
            batcher.push(AgentEventKind::ThinkingDelta {
                message_id,
                block: index,
                text,
            });
        }
        StreamEvent::ThinkingSignature { index, signature } => {
            if let Some(ContentPart::Thinking { signature: s, .. }) = parts.get_mut(&index) {
                *s = Some(signature);
            }
        }
        StreamEvent::ToolCallStart { .. }
        | StreamEvent::ToolCallArgsDelta { .. }
        | StreamEvent::ToolCallEnd { .. } => {
            // Tools arrive with M3; until then a model that calls one gets a notice.
            batcher.push(AgentEventKind::ProviderNotice {
                kind: "tool_call_ignored".into(),
                detail: "the model tried to call a tool; tools arrive in a later milestone".into(),
            });
        }
        StreamEvent::ProviderBlock { index, part } => {
            parts.insert(index, part);
            sync_state(active, parts);
        }
        StreamEvent::Usage(u) => *usage = Some(u),
        StreamEvent::MessageEnd { stop_reason } => *end = Some(End::Completed(stop_reason)),
    }
}

fn sync_state(active: &ActiveTurn, parts: &BTreeMap<u32, ContentPart>) {
    let mut s = active.state.lock().unwrap_or_else(|e| e.into_inner());
    s.parts = parts.clone();
}
