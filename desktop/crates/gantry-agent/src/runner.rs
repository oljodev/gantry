//! One turn (docs/plan/01 §3 steps 2 to 7): build the request, stream, convert provider events
//! to agent events, then, when the model stopped with tool calls, decide, execute, append the
//! results and go round again until it stops without calls, the round cap hits, the user
//! cancels, or an error occurs.

use std::{collections::BTreeMap, sync::Arc, time::Instant};

use futures_util::StreamExt;
use gantry_connectors::{ChatScope, ToolCallRequest, ToolEventSink, ToolOutcome};
use gantry_core::{
    AgentEventKind, CallId, ContentPart, DecisionSource, GrantScope, GrantSource, Interaction,
    InteractionPayload, InteractionResolution, Message, MessageId, PermissionDecision,
    PermissionRequest, ProviderErrorKind, ProviderKind, ResultPart, RiskTier, Role, StopReason,
    ToolCallDto, ToolCallStatus, TurnStatus, Usage, now_ms, result_preview,
};
use gantry_providers::{ChatRequest, Provider, ProviderError, ServerTool, StreamEvent};
use serde_json::json;

use crate::{
    chats::{ChatBook, TurnInput, TurnOutcome},
    interactions::Interactions,
    permissions::{self, Decision},
    tools::{ToolEntry, ToolSet, display_for},
    turn_manager::{ActiveTurn, ChatNotifier, LiveMessage},
};

/// Result previews in rows and the projection stop here.
pub const PREVIEW_CHARS: usize = 2_000;
/// What the model receives of one result, at most (05 §8).
pub const RESULT_MAX_BYTES: usize = 50 * 1024;

pub struct RunContext {
    pub input: TurnInput,
    pub provider: Option<Arc<dyn Provider>>,
    pub max_output_tokens: u32,
    pub max_tool_rounds: u32,
    pub active: Arc<ActiveTurn>,
    pub chats: Arc<ChatBook>,
    pub tools: ToolSet,
    pub interactions: Arc<Interactions>,
    pub notifier: Option<Arc<dyn ChatNotifier>>,
}

enum End {
    Completed(StopReason),
    Cancelled,
    Failed(ProviderError),
}

struct Round {
    message_id: MessageId,
    parts: BTreeMap<u32, ContentPart>,
    usage: Option<Usage>,
    end: End,
}

struct Call {
    id: CallId,
    name: String,
    args: serde_json::Value,
}

pub async fn run_turn(ctx: RunContext) {
    let started = Instant::now();
    let batcher = ctx.active.batcher.clone();
    batcher.push(AgentEventKind::TurnStarted {
        chat_id: ctx.input.chat_id,
        mode: ctx.input.mode,
        guard: ctx.input.guard,
        model: ctx.input.model.clone(),
    });

    let mut transcript = ctx.input.messages.clone();
    if let Some(detail) = thinking_reset_notice(&ctx.input) {
        batcher.push(AgentEventKind::ProviderNotice {
            kind: "thinking_dropped".into(),
            detail,
        });
    }
    let mut usage_total: Option<Usage> = None;
    let mut rounds: u32 = 0;
    let mut call_count: u32 = 0;
    let (status, stop_reason, error) = loop {
        let round = stream_round(&ctx, &transcript).await;
        usage_total = match (usage_total, round.usage) {
            (Some(a), Some(b)) => Some(a.plus(b)),
            (a, b) => a.or(b),
        };
        for (block, part) in &round.parts {
            batcher.push(AgentEventKind::BlockDone {
                message_id: round.message_id,
                block: *block,
                part: part.clone(),
            });
        }
        let assistant = Message {
            id: round.message_id,
            role: Role::Assistant,
            parts: round.parts.values().cloned().collect(),
            origin: ctx.provider.as_ref().map(|p| p.kind()),
            created_at: now_ms(),
        };
        let calls: Vec<Call> = assistant
            .parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::ToolCall { id, name, args, .. } => Some(Call {
                    id: id.clone(),
                    name: name.clone(),
                    args: args.clone(),
                }),
                _ => None,
            })
            .collect();

        match round.end {
            End::Failed(err) => {
                if !assistant.parts.is_empty() {
                    ctx.chats.append_turn_message(
                        ctx.input.chat_id,
                        ctx.input.turn_id,
                        assistant.clone(),
                        None,
                        round.usage,
                    );
                    close_unrun_calls(&ctx, &calls, ToolCallStatus::Failed, &err.message);
                }
                batcher.push(AgentEventKind::Error {
                    code: format!("{:?}", err.kind).to_ascii_lowercase(),
                    message: err.message.clone(),
                    retryable: err.kind.is_retryable(),
                });
                break (TurnStatus::Failed, None, Some(err.message));
            }
            End::Cancelled => {
                if !assistant.parts.is_empty() {
                    ctx.chats.append_turn_message(
                        ctx.input.chat_id,
                        ctx.input.turn_id,
                        assistant.clone(),
                        Some(StopReason::Cancelled),
                        round.usage,
                    );
                    close_unrun_calls(&ctx, &calls, ToolCallStatus::Cancelled, CANCELLED_RESULT);
                }
                batcher.push(AgentEventKind::MessageCompleted {
                    message_id: round.message_id,
                    stop_reason: StopReason::Cancelled,
                    usage: round.usage,
                });
                break (TurnStatus::Cancelled, Some(StopReason::Cancelled), None);
            }
            End::Completed(reason) => {
                batcher.push(AgentEventKind::MessageCompleted {
                    message_id: round.message_id,
                    stop_reason: reason.clone(),
                    usage: round.usage,
                });
                if !assistant.parts.is_empty() {
                    ctx.chats.append_turn_message(
                        ctx.input.chat_id,
                        ctx.input.turn_id,
                        assistant.clone(),
                        Some(reason.clone()),
                        round.usage,
                    );
                }
                if calls.is_empty() {
                    break (TurnStatus::Completed, Some(reason), None);
                }
                rounds += 1;
                call_count += u32::try_from(calls.len()).unwrap_or(u32::MAX);
                transcript.push(assistant.clone());
                let capped = rounds > ctx.max_tool_rounds;
                let (results, cancelled) = if capped {
                    let detail = format!(
                        "Gantry stopped this reply after {} tool rounds (Settings → Advanced).",
                        ctx.max_tool_rounds
                    );
                    close_unrun_calls(&ctx, &calls, ToolCallStatus::Cancelled, &detail);
                    (synthetic_results(&calls, &detail), false)
                } else {
                    run_calls(&ctx, &assistant, &calls).await
                };
                let tool_message = Message {
                    id: MessageId::new(),
                    role: Role::Tool,
                    parts: results,
                    origin: None,
                    created_at: now_ms(),
                };
                ctx.chats.append_turn_message(
                    ctx.input.chat_id,
                    ctx.input.turn_id,
                    tool_message.clone(),
                    None,
                    None,
                );
                {
                    let mut s = ctx.active.state.lock().unwrap_or_else(|e| e.into_inner());
                    s.messages.push(LiveMessage::finished(&tool_message));
                }
                transcript.push(tool_message);
                if cancelled {
                    break (TurnStatus::Cancelled, Some(StopReason::Cancelled), None);
                }
                if capped {
                    batcher.push(AgentEventKind::ProviderNotice {
                        kind: "tool_round_cap".into(),
                        detail: format!(
                            "Stopped after {} tool rounds; raise the cap in Settings → Advanced if this reply needed more.",
                            ctx.max_tool_rounds
                        ),
                    });
                    break (
                        TurnStatus::Completed,
                        Some(StopReason::Other {
                            reason: "max_tool_rounds".into(),
                        }),
                        None,
                    );
                }
            }
        }
    };

    {
        let mut s = ctx.active.state.lock().unwrap_or_else(|e| e.into_inner());
        s.status = status;
        s.usage = usage_total;
    }
    // Hand every queued event to the persister now, so its detached writes sit ahead of the
    // blocking one below on the store's writer: when the turn reads as finished, the
    // tool_calls and interactions rows are already there.
    batcher.flush();
    ctx.chats.finish_turn(
        ctx.input.chat_id,
        ctx.input.turn_id,
        TurnOutcome {
            status,
            usage: usage_total,
            stop_reason,
            error,
            tool_call_count: call_count,
        },
    );
    batcher.push(AgentEventKind::TurnCompleted {
        status,
        usage: usage_total,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        tool_calls: call_count,
    });
    batcher.close();
}

/// One model request: streams until the message ends, cancel trips, or the provider fails.
async fn stream_round(ctx: &RunContext, transcript: &[Message]) -> Round {
    let batcher = ctx.active.batcher.clone();
    let message_id = MessageId::new();
    {
        let mut s = ctx.active.state.lock().unwrap_or_else(|e| e.into_inner());
        s.messages.push(LiveMessage {
            id: message_id,
            role: Role::Assistant,
            parts: BTreeMap::new(),
        });
    }
    batcher.push(AgentEventKind::MessageStarted {
        message_id,
        role: Role::Assistant,
    });
    let mut round = Round {
        message_id,
        parts: BTreeMap::new(),
        usage: None,
        end: End::Cancelled,
    };
    let Some(provider) = ctx.provider.clone() else {
        round.end = End::Failed(ProviderError::new(
            ProviderErrorKind::NotFound,
            format!("provider {} is not configured", ctx.input.model.provider),
        ));
        return round;
    };
    let mut req = ChatRequest::new(
        ctx.input.model.model.clone(),
        ctx.input.system.clone(),
        transcript.to_vec(),
    );
    req.max_output_tokens = ctx.max_output_tokens;
    req.reasoning = ctx.input.effort;
    req.tools = ctx.tools.specs();
    if ctx.input.web_search {
        req.server_tools = vec![ServerTool::WebSearch { max_uses: None }];
    }
    req.metadata.chat_id = Some(ctx.input.chat_id);
    req.metadata.turn_id = Some(ctx.input.turn_id);
    let cancel = ctx.active.cancel.clone();
    let opened = tokio::select! {
        _ = cancel.cancelled() => Err(None),
        r = provider.stream(req) => r.map_err(Some),
    };
    let mut stream = match opened {
        Err(None) => return round,
        Err(Some(err)) => {
            round.end = End::Failed(err);
            return round;
        }
        Ok(s) => s,
    };
    let mut raw_args: BTreeMap<u32, String> = BTreeMap::new();
    let mut end: Option<End> = None;
    while end.is_none() {
        let next = tokio::select! {
            _ = cancel.cancelled() => { end = Some(End::Cancelled); break; }
            n = stream.next() => n,
        };
        match next {
            None => {
                end = Some(End::Failed(ProviderError::interrupted(
                    "the stream ended without a stop reason",
                )));
            }
            Some(Err(err)) => end = Some(End::Failed(err)),
            Some(Ok(ev)) => apply(
                ctx,
                ev,
                &mut round,
                &mut raw_args,
                provider.kind(),
                &mut end,
            ),
        }
    }
    round.end = end.unwrap_or(End::Cancelled);
    round
}

fn apply(
    ctx: &RunContext,
    ev: StreamEvent,
    round: &mut Round,
    raw_args: &mut BTreeMap<u32, String>,
    provider: ProviderKind,
    end: &mut Option<End>,
) {
    let batcher = &ctx.active.batcher;
    let message_id = round.message_id;
    match ev {
        StreamEvent::MessageStart { .. } => {}
        StreamEvent::TextDelta { index, text } => {
            if let ContentPart::Text { text: t } =
                round
                    .parts
                    .entry(index)
                    .or_insert_with(|| ContentPart::Text {
                        text: String::new(),
                    })
            {
                t.push_str(&text);
            }
            sync_parts(ctx, round);
            batcher.push(AgentEventKind::TextDelta {
                message_id,
                block: index,
                text,
            });
        }
        StreamEvent::ThinkingDelta { index, text } => {
            if let ContentPart::Thinking { text: t, .. } =
                round
                    .parts
                    .entry(index)
                    .or_insert_with(|| ContentPart::Thinking {
                        item_id: None,
                        text: String::new(),
                        signature: None,
                        provider,
                    })
            {
                t.push_str(&text);
            }
            sync_parts(ctx, round);
            batcher.push(AgentEventKind::ThinkingDelta {
                message_id,
                block: index,
                text,
            });
        }
        StreamEvent::ThinkingSignature { index, signature } => {
            if let Some(ContentPart::Thinking { signature: s, .. }) = round.parts.get_mut(&index) {
                *s = Some(signature);
            }
        }
        StreamEvent::ToolCallStart { index, id, name } => {
            let id = if id.as_str().is_empty() {
                CallId::new()
            } else {
                id
            };
            round.parts.insert(
                index,
                ContentPart::ToolCall {
                    signature: None,
                    id: id.clone(),
                    name: name.clone(),
                    args: serde_json::Value::Null,
                },
            );
            let entry = ctx.tools.resolve(&name);
            let (connector, tool) = match entry {
                Some(e) => (e.connector_id().to_owned(), e.def.name.clone()),
                None => ToolSet::split_name(&name),
            };
            let connector_name = entry
                .map(|e| e.connector_name().to_owned())
                .unwrap_or_else(|| connector.clone());
            let dto = ToolCallDto {
                id: id.clone(),
                chat_id: ctx.input.chat_id,
                turn_id: ctx.input.turn_id,
                message_id,
                connector: connector.clone(),
                connector_name: connector_name.clone(),
                tool: tool.clone(),
                model_tool_name: name.clone(),
                args: serde_json::Value::Null,
                tier: entry.map(|e| e.def.tier).unwrap_or(RiskTier::Read),
                status: ToolCallStatus::Proposed,
                decision_source: None,
                display: display_for(entry.map(|e| &e.def), &serde_json::Value::Null),
                result_preview: None,
                result: None,
                is_error: false,
                started_at: None,
                ended_at: None,
                duration_ms: None,
            };
            {
                let mut s = ctx.active.state.lock().unwrap_or_else(|e| e.into_inner());
                s.tool_calls.push(dto);
            }
            sync_parts(ctx, round);
            batcher.push(AgentEventKind::ToolCallStarted {
                call_id: id,
                message_id,
                connector,
                connector_name,
                tool,
                model_tool_name: name,
            });
        }
        StreamEvent::ToolCallArgsDelta {
            index,
            json_fragment,
        } => {
            raw_args.entry(index).or_default().push_str(&json_fragment);
            if let Some(ContentPart::ToolCall { id, .. }) = round.parts.get(&index) {
                batcher.push(AgentEventKind::ToolCallArgsDelta {
                    call_id: id.clone(),
                    fragment: json_fragment,
                });
            }
        }
        StreamEvent::ToolCallEnd { index, args } => {
            let args = if args.is_object() { args } else { json!({}) };
            // The observed column of 13 §2: whether this provider streamed the arguments.
            log::info!(
                "tool arguments from {:?} · {}: {}",
                provider,
                ctx.input.model.model,
                if raw_args.get(&index).is_some_and(|s| !s.is_empty()) {
                    "streamed in fragments"
                } else {
                    "arrived whole"
                }
            );
            if let Some(ContentPart::ToolCall {
                id,
                name,
                args: slot,
                ..
            }) = round.parts.get_mut(&index)
            {
                *slot = args.clone();
                let entry = ctx.tools.resolve(name);
                let tier = entry.map(|e| e.def.tier).unwrap_or(RiskTier::Read);
                let display = display_for(entry.map(|e| &e.def), &args);
                update_call(ctx, id, |c| {
                    c.args = args.clone();
                    c.tier = tier;
                    c.display = display.clone();
                });
                batcher.push(AgentEventKind::ToolCallReady {
                    call_id: id.clone(),
                    args,
                    tier,
                    display,
                });
            }
            sync_parts(ctx, round);
        }
        StreamEvent::ProviderBlock { index, part } => {
            round.parts.insert(index, part);
            sync_parts(ctx, round);
        }
        StreamEvent::Usage(u) => round.usage = Some(u),
        StreamEvent::MessageEnd { stop_reason } => *end = Some(End::Completed(stop_reason)),
    }
}

fn sync_parts(ctx: &RunContext, round: &Round) {
    let mut s = ctx.active.state.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(m) = s.messages.iter_mut().find(|m| m.id == round.message_id) {
        m.parts = round.parts.clone();
    }
}

fn update_call(ctx: &RunContext, id: &CallId, f: impl FnOnce(&mut ToolCallDto)) {
    let mut s = ctx.active.state.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = s.tool_calls.iter_mut().find(|c| &c.id == id) {
        f(c);
    }
}

pub const CANCELLED_RESULT: &str = "Cancelled by the user before this tool call ran.";

/// The "Thinking context reset" notice (05 §1): the chat's model changed since the last turn,
/// and the last reply's thinking belongs to the previous one, so it is not sent along (02 §5).
fn thinking_reset_notice(input: &TurnInput) -> Option<String> {
    let previous = input.previous_model.as_ref()?;
    if previous == &input.model {
        return None;
    }
    let last_reply = input
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant)?;
    let had_thinking = last_reply
        .parts
        .iter()
        .any(|p| matches!(p, ContentPart::Thinking { .. }));
    had_thinking.then(|| {
        format!(
            "Thinking context reset: the reasoning {} did earlier is not sent to {}.",
            previous.model, input.model.model
        )
    })
}

/// Error results for calls that will not run, so the transcript stays replayable (02 §3).
fn synthetic_results(calls: &[Call], text: &str) -> Vec<ContentPart> {
    calls
        .iter()
        .map(|c| ContentPart::ToolResult {
            call_id: c.id.clone(),
            content: vec![ResultPart::Text {
                text: text.to_owned(),
            }],
            is_error: true,
        })
        .collect()
}

/// Marks calls that never ran as over, in the live state and on the channel.
fn close_unrun_calls(ctx: &RunContext, calls: &[Call], status: ToolCallStatus, detail: &str) {
    for c in calls {
        complete_call(
            ctx,
            &c.id,
            status,
            true,
            0,
            vec![ResultPart::Text {
                text: detail.to_owned(),
            }],
        );
    }
}

fn complete_call(
    ctx: &RunContext,
    id: &CallId,
    status: ToolCallStatus,
    is_error: bool,
    duration_ms: u64,
    result: Vec<ResultPart>,
) -> ContentPart {
    let preview = result_preview(&result, PREVIEW_CHARS);
    update_call(ctx, id, |c| {
        c.status = status;
        c.is_error = is_error;
        c.result_preview = Some(preview.clone());
        c.result = Some(result.clone());
        c.ended_at = Some(now_ms());
        c.duration_ms = Some(duration_ms);
    });
    ctx.active.batcher.push(AgentEventKind::ToolCallCompleted {
        call_id: id.clone(),
        status,
        is_error,
        duration_ms,
        result_preview: preview,
        result: result.clone(),
    });
    ContentPart::ToolResult {
        call_id: id.clone(),
        content: result,
        is_error,
    }
}

fn denied_result(message: Option<String>) -> Vec<ResultPart> {
    vec![ResultPart::Json {
        json: json!({
            "error": "denied_by_user",
            "message": message.unwrap_or_else(|| "The user did not allow this call.".to_owned()),
            "hint": "Ask the user or choose a safer approach."
        }),
    }]
}

/// Steps 4 and 5 of 01 §3 for one batch of calls: decide each (asking the user where the mode
/// says so, all prompts at once so they stack), execute the allowed ones, and return one
/// result per call in the model's order plus whether the user cancelled meanwhile.
async fn run_calls(
    ctx: &RunContext,
    assistant: &Message,
    calls: &[Call],
) -> (Vec<ContentPart>, bool) {
    let batcher = ctx.active.batcher.clone();
    let mut results: Vec<Option<ContentPart>> = (0..calls.len()).map(|_| None).collect();
    let mut allowed: Vec<(usize, ToolEntry, DecisionSource)> = Vec::new();
    let mut waiting: Vec<(
        usize,
        ToolEntry,
        Interaction,
        tokio::sync::oneshot::Receiver<InteractionResolution>,
    )> = Vec::new();
    let why = last_sentence(&assistant.text());
    // The chat's standing grants, read once for this batch (04 §8). A grant made in answer to
    // one card applies from the next batch, which is where the model asks again anyway.
    let grants = ctx.chats.grants(ctx.input.chat_id).unwrap_or_else(|err| {
        log::warn!("could not read the chat's grants: {err}");
        Vec::new()
    });

    for (i, call) in calls.iter().enumerate() {
        let Some(entry) = ctx.tools.resolve(&call.name).cloned() else {
            results[i] = Some(complete_call(
                ctx,
                &call.id,
                ToolCallStatus::Failed,
                true,
                0,
                vec![ResultPart::Text {
                    text: format!(
                        "Unknown tool `{}`. Use one of the tools you were given.",
                        call.name
                    ),
                }],
            ));
            continue;
        };
        let decision = permissions::decide(
            ctx.input.mode,
            ctx.input.guard,
            &permissions::Call {
                def: &entry.def,
                instance_id: entry.connector_id(),
                args: &call.args,
            },
            &grants,
        );
        match decision {
            Decision::Allow(source) => allowed.push((i, entry, source)),
            Decision::Deny { source, reason } => {
                update_call(ctx, &call.id, |c| c.decision_source = Some(source));
                results[i] = Some(complete_call(
                    ctx,
                    &call.id,
                    ToolCallStatus::Denied,
                    true,
                    0,
                    vec![ResultPart::Json {
                        json: json!({ "error": "denied_by_policy", "message": reason }),
                    }],
                ));
            }
            Decision::Ask => {
                let interaction = Interaction::pending(
                    ctx.input.chat_id,
                    ctx.input.turn_id,
                    InteractionPayload::Permission {
                        request: PermissionRequest {
                            call_id: call.id.clone(),
                            connector: entry.connector_id().to_owned(),
                            connector_name: entry.connector_name().to_owned(),
                            tool: entry.def.name.clone(),
                            model_tool_name: entry.model_name.clone(),
                            tier: entry.def.tier,
                            args: call.args.clone(),
                            display: display_for(Some(&entry.def), &call.args),
                            why: why.clone(),
                            description: entry.def.description.clone(),
                            scopes: GrantScope::for_tier(entry.def.tier),
                        },
                    },
                );
                let rx = ctx.interactions.request(interaction.clone());
                update_call(ctx, &call.id, |c| {
                    c.status = ToolCallStatus::AwaitingDecision
                });
                {
                    let mut s = ctx.active.state.lock().unwrap_or_else(|e| e.into_inner());
                    s.pending.push(interaction.clone());
                }
                batcher.push(AgentEventKind::DecisionRequested {
                    interaction: Box::new(interaction.clone()),
                });
                waiting.push((i, entry, interaction, rx));
            }
        }
    }

    if !waiting.is_empty() {
        notify_pending(ctx);
    }
    let mut cancelled = false;
    for (i, entry, interaction, rx) in waiting {
        let call = &calls[i];
        let resolution = if cancelled {
            InteractionResolution::Cancelled
        } else {
            tokio::select! {
                _ = ctx.active.cancel.cancelled() => InteractionResolution::Cancelled,
                r = rx => r.unwrap_or(InteractionResolution::Cancelled),
            }
        };
        {
            let mut s = ctx.active.state.lock().unwrap_or_else(|e| e.into_inner());
            s.pending.retain(|p| p.id != interaction.id);
        }
        match resolution {
            InteractionResolution::Permission { decision, .. } if decision.allows() => {
                // "Allow for this chat" is remembered before the call runs, so a crash in the
                // middle of the call cannot lose the answer the user just gave (04 §8).
                let source = match decision {
                    PermissionDecision::AllowChat { scope } => {
                        let grant = scope.grant(
                            ctx.input.chat_id,
                            entry.connector_id(),
                            entry.connector_name(),
                            &entry.def.name,
                            GrantSource::UserPrompt,
                        );
                        match ctx.chats.add_grant(grant) {
                            Ok(()) => DecisionSource::UserChatGrant,
                            Err(err) => {
                                log::warn!("could not store the grant: {err}");
                                DecisionSource::UserOnce
                            }
                        }
                    }
                    _ => DecisionSource::UserOnce,
                };
                batcher.push(AgentEventKind::DecisionResolved {
                    interaction_id: interaction.id,
                    resolution,
                    source,
                });
                allowed.push((i, entry, source));
            }
            // Everything the guard above did not take is a denial.
            InteractionResolution::Permission { message, .. } => {
                batcher.push(AgentEventKind::DecisionResolved {
                    interaction_id: interaction.id,
                    resolution: InteractionResolution::Permission {
                        decision: PermissionDecision::Deny,
                        message: message.clone(),
                    },
                    source: DecisionSource::UserOnce,
                });
                update_call(ctx, &call.id, |c| {
                    c.decision_source = Some(DecisionSource::UserOnce)
                });
                results[i] = Some(complete_call(
                    ctx,
                    &call.id,
                    ToolCallStatus::Denied,
                    true,
                    0,
                    denied_result(message),
                ));
            }
            InteractionResolution::Cancelled => {
                cancelled = true;
                ctx.interactions.cancel_turn(ctx.input.turn_id);
                batcher.push(AgentEventKind::DecisionResolved {
                    interaction_id: interaction.id,
                    resolution: InteractionResolution::Cancelled,
                    source: DecisionSource::UserOnce,
                });
                results[i] = Some(complete_call(
                    ctx,
                    &call.id,
                    ToolCallStatus::Cancelled,
                    true,
                    0,
                    vec![ResultPart::Text {
                        text: CANCELLED_RESULT.to_owned(),
                    }],
                ));
            }
        }
        notify_pending(ctx);
    }

    if cancelled {
        for (i, _, _) in allowed {
            results[i] = Some(complete_call(
                ctx,
                &calls[i].id,
                ToolCallStatus::Cancelled,
                true,
                0,
                vec![ResultPart::Text {
                    text: CANCELLED_RESULT.to_owned(),
                }],
            ));
        }
    } else {
        // Parallel when every call of the batch says it is safe; otherwise in the model's order.
        let parallel = allowed.iter().all(|(_, e, _)| e.def.parallel_safe);
        for (i, entry, source) in &allowed {
            update_call(ctx, &calls[*i].id, |c| {
                c.status = ToolCallStatus::Running;
                c.decision_source = Some(*source);
                c.started_at = Some(now_ms());
            });
            batcher.push(AgentEventKind::ToolCallExecuting {
                call_id: calls[*i].id.clone(),
                source: *source,
            });
            let _ = entry;
        }
        if parallel {
            let futures = allowed
                .iter()
                .map(|(i, entry, _)| execute(ctx, &calls[*i], entry.clone()));
            let outcomes = futures_util::future::join_all(futures).await;
            for ((i, _, _), part) in allowed.iter().zip(outcomes) {
                results[*i] = Some(part);
            }
        } else {
            for (i, entry, _) in &allowed {
                results[*i] = Some(execute(ctx, &calls[*i], entry.clone()).await);
            }
        }
        cancelled = ctx.active.cancel.is_cancelled();
    }

    let results = results
        .into_iter()
        .zip(calls)
        .map(|(r, c)| {
            r.unwrap_or_else(|| ContentPart::ToolResult {
                call_id: c.id.clone(),
                content: vec![ResultPart::Text {
                    text: CANCELLED_RESULT.to_owned(),
                }],
                is_error: true,
            })
        })
        .collect();
    (results, cancelled)
}

/// Runs one allowed call to its result part; cancellation yields an error result.
async fn execute(ctx: &RunContext, call: &Call, entry: ToolEntry) -> ContentPart {
    let started = Instant::now();
    let req = ToolCallRequest {
        call_id: call.id.clone(),
        tool: entry.def.name.clone(),
        args: call.args.clone(),
        scope: ChatScope {
            chat_id: ctx.input.chat_id,
            mode: ctx.input.mode,
        },
    };
    let cancel = ctx.active.cancel.child_token();
    let sink = Arc::new(TurnToolEvents {
        batcher: ctx.active.batcher.clone(),
    });
    let outcome = tokio::select! {
        _ = ctx.active.cancel.cancelled() => None,
        r = entry.connector.call(req, sink, cancel) => Some(r),
    };
    let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (status, is_error, content) = match outcome {
        None => (
            ToolCallStatus::Cancelled,
            true,
            vec![ResultPart::Text {
                text: CANCELLED_RESULT.to_owned(),
            }],
        ),
        Some(Err(err)) => (
            ToolCallStatus::Failed,
            true,
            vec![ResultPart::Text {
                text: err.to_string(),
            }],
        ),
        Some(Ok(ToolOutcome::Complete {
            content, is_error, ..
        })) => (ToolCallStatus::Completed, is_error, cap_result(content)),
    };
    complete_call(ctx, &call.id, status, is_error, elapsed, content)
}

/// The sink a running call reports through: events a runtime tool produces itself
/// (`artifact.*`) join the turn stream; output and progress arrive with the shell (M7).
struct TurnToolEvents {
    batcher: Arc<crate::events::Batcher>,
}

impl ToolEventSink for TurnToolEvents {
    fn event(&self, event: AgentEventKind) {
        self.batcher.push(event);
    }
}

/// Keeps a result under the transcript limit: head and tail with a marker between (05 §8).
fn cap_result(content: Vec<ResultPart>) -> Vec<ResultPart> {
    let size: usize = content
        .iter()
        .map(|p| match p {
            ResultPart::Text { text } => text.len(),
            ResultPart::Json { json } => json.to_string().len(),
            ResultPart::Image { data, .. } => data.len(),
            ResultPart::Resource { summary, .. } => summary.len(),
        })
        .sum();
    if size <= RESULT_MAX_BYTES {
        return content;
    }
    let text = result_preview(&content, usize::MAX);
    let half = RESULT_MAX_BYTES / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(half)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    vec![ResultPart::Text {
        text: format!(
            "{head}\n\n[… {} characters omitted …]\n\n{tail}",
            text.chars().count().saturating_sub(2 * half)
        ),
    }]
}

fn notify_pending(ctx: &RunContext) {
    if let Some(n) = &ctx.notifier {
        n.interactions_changed(
            ctx.input.chat_id,
            ctx.interactions.pending_count(ctx.input.chat_id),
        );
    }
}

/// The assistant's last sentence before a call, as the card's "why" (04 §7).
fn last_sentence(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let last = trimmed
        .rsplit_terminator(['.', '!', '?', '\n'])
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or(trimmed);
    let short: String = last.chars().take(200).collect();
    Some(short)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_why_is_the_last_sentence() {
        assert_eq!(
            last_sentence("I will check the time. Let me look it up"),
            Some("Let me look it up".into())
        );
        assert_eq!(last_sentence("   "), None);
    }

    #[test]
    fn oversized_results_keep_head_and_tail() {
        let big = "x".repeat(RESULT_MAX_BYTES + 100);
        let capped = cap_result(vec![ResultPart::Text { text: big }]);
        let ResultPart::Text { text } = &capped[0] else {
            panic!()
        };
        assert!(text.contains("characters omitted"));
        assert!(text.len() < RESULT_MAX_BYTES + 100);
    }
}
