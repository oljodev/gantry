//! One turn (docs/plan/01 §3 steps 2 to 7): build the request, stream, convert provider events
//! to agent events, then, when the model stopped with tool calls, decide, execute, append the
//! results and go round again until it stops without calls, the round cap hits, the user
//! cancels, or an error occurs.

use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
    time::Instant,
};

use futures_util::StreamExt;
use gantry_connectors::{
    ChatScope, ConnectorRegistry, ElicitationAnswer, OutputStream, ToolCallRequest, ToolEventSink,
    ToolOutcome,
};
use gantry_core::{
    AgentEventKind, CallId, ContentPart, DecisionSource, GrantScope, GrantSource, Interaction,
    InteractionPayload, InteractionResolution, JudgeDecision, JudgeSource, JudgeVerdict, Message,
    MessageId, PermissionDecision, PermissionRequest, ProviderErrorKind, ProviderKind, ResultPart,
    RiskTier, Role, StopReason, ToolCallDto, ToolCallStatus, TurnStatus, Usage, now_ms,
    result_preview,
};
use gantry_providers::{ChatRequest, Provider, ProviderError, ServerTool, StreamEvent};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::{
    chats::{ChatBook, TurnInput, TurnOutcome},
    context,
    interactions::Interactions,
    judge,
    permissions::{self, Decision},
    tools::{ToolEntry, ToolSet, display_for},
    turn_manager::{ActiveTurn, ChatNotifier, LiveMessage},
};

/// Result previews in rows and the projection stop here.
pub const PREVIEW_CHARS: usize = 2_000;
/// What the model receives of one result when nobody has said otherwise (05 §8); the setting
/// that overrides it is `advanced.max_result_kb`.
pub const RESULT_MAX_BYTES: usize = 50 * 1024;

pub struct RunContext {
    pub input: TurnInput,
    pub provider: Option<Arc<dyn Provider>>,
    pub max_output_tokens: u32,
    pub max_tool_rounds: u32,
    /// What one tool result may contribute to the transcript (05 §8).
    pub max_result_bytes: usize,
    /// What the user chose for this chat's model, where it makes something other than text.
    pub media: gantry_core::MediaOptions,
    /// The floor of 04 §5, compiled once for the turn: the rules no mode and no grant lifts.
    pub guardrails: Arc<gantry_core::Guardrails>,
    /// Who the guard asks in Guarded Auto (04 §6): by default the cheapest fast model of the
    /// chat's own provider, so no second key is needed. `None` when there is no provider to
    /// ask, and then every guarded call falls back to the user.
    pub judge: Option<(Arc<dyn Provider>, String)>,
    /// The blocks the user overrode with **Allow anyway** (04 §6), shared with the manager
    /// because the button is pressed after the turn that was blocked has ended.
    pub overrides: Arc<judge::Overrides>,
    pub active: Arc<ActiveTurn>,
    pub chats: Arc<ChatBook>,
    /// The tools of this turn. Behind a lock because attaching a connector mid-turn (04 §9)
    /// rebuilds it between rounds.
    pub tools: RwLock<ToolSet>,
    pub connectors: Arc<ConnectorRegistry>,
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

/// What the guard remembers across the rounds of one turn (04 §6): the failures that make a
/// loop, the calls it has already seen, and how many it has refused. It lives for the turn and
/// no longer — the next turn is a new task, and a call that failed three times before the user
/// last spoke deserves to be tried once more.
#[derive(Default)]
struct GuardState {
    loops: judge::LoopTracker,
    recent: Vec<judge::Recent>,
    denials: u32,
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

    // What this turn was given beyond the transcript (05 §2, 12). It is emitted before the
    // first token, so the "Context used" row is above the answer it shaped rather than under
    // it.
    if !ctx.input.injected.is_empty() {
        batcher.push(AgentEventKind::ContextInjected {
            injected: ctx.input.injected.clone(),
        });
    }

    let mut transcript = ctx.input.messages.clone();
    let mut attached = ctx.input.connectors.clone();
    if let Some(detail) = thinking_reset_notice(&ctx.input) {
        batcher.push(AgentEventKind::ProviderNotice {
            kind: "thinking_dropped".into(),
            detail,
        });
    }
    let mut usage_total: Option<Usage> = None;
    // The *last* round's usage, not the sum: it is the size of the prompt the provider just
    // charged for, which is what the next request's prefix will be (02 §6).
    let mut last_usage: Option<Usage> = None;
    let mut rounds: u32 = 0;
    let mut call_count: u32 = 0;
    let mut guard = GuardState::default();
    let (status, stop_reason, error) = loop {
        let round = stream_round(&ctx, &transcript).await;
        if round.usage.is_some() {
            last_usage = round.usage;
        }
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
                    transcript.push(assistant.clone());
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
                    run_calls(&ctx, &assistant, &calls, &mut guard).await
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
                if let Some(change) = refresh_tools(&ctx, &mut attached).await {
                    ctx.chats.append_turn_message(
                        ctx.input.chat_id,
                        ctx.input.turn_id,
                        change.clone(),
                        None,
                        None,
                    );
                    {
                        let mut s = ctx.active.state.lock().unwrap_or_else(|e| e.into_inner());
                        s.messages.push(LiveMessage::finished(&change));
                    }
                    transcript.push(change);
                }
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

    // After the answer is on screen, not before it (02 §6). The turn is finished and its events
    // are closed; the marker this may append reaches the interface with the chat's next refresh,
    // where it reads as the row it is rather than as a pause at the end of a reply.
    if status == TurnStatus::Completed {
        compact_if_needed(&ctx, &transcript, last_usage.as_ref()).await;
    }
}

/// Summarizes the older part of the chat when the next request would come too close to the
/// model's window (02 §6). Nothing is removed: one `System` message is appended saying what the
/// summary stands for, and the projection of every later turn skips those messages.
///
/// Every failure here is survivable and none of them is worth telling the user about at the end
/// of an answer that worked: without a marker the next turn simply sends more, and if that is
/// genuinely too much the provider says so with a `ContextTooLong` the user can act on.
async fn compact_if_needed(ctx: &RunContext, transcript: &[Message], last: Option<&Usage>) {
    let Some(provider) = ctx.provider.clone() else {
        return;
    };
    let specs = ctx.tools.read().unwrap_or_else(|e| e.into_inner()).specs();
    let overhead = context::overhead(&ctx.input.system, &specs);
    let estimate = context::estimate(transcript, overhead, last);
    let window = provider
        .model_info(&ctx.input.model.model)
        .and_then(|m| m.context_window);
    if !context::over_budget(estimate, window) {
        return;
    }
    let keep = context::keep_turns(provider.kind());
    let Some(span) = context::span(transcript, keep) else {
        log::info!(
            "chat {} is at ~{estimate} tokens but has nothing older to summarize",
            ctx.input.chat_id
        );
        return;
    };
    let messages = &transcript[span.clone()];
    let Some(up_to) = messages.last().map(|m| m.id) else {
        return;
    };
    let artifacts = context::artifacts_in(messages);
    let model = crate::title::judge_model(
        ctx.input.model.provider.as_str(),
        provider.kind(),
        &ctx.input.model.model,
    );
    log::info!(
        "compacting chat {}: ~{estimate} tokens, summarizing {} of {} messages",
        ctx.input.chat_id,
        messages.len(),
        transcript.len()
    );
    match context::summarize(provider, model, messages, &artifacts).await {
        Ok(summary) => {
            let marker = context::marker(summary, up_to, messages.len(), artifacts);
            ctx.chats
                .append_turn_message(ctx.input.chat_id, ctx.input.turn_id, marker, None, None);
            if let Some(notifier) = &ctx.notifier {
                notifier.chats_changed(vec![ctx.input.chat_id]);
            }
        }
        Err(err) => log::warn!("could not summarize chat {}: {err}", ctx.input.chat_id),
    }
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
    req.media = ctx.media.clone();
    req.tools = ctx.tools.read().unwrap_or_else(|e| e.into_inner()).specs();
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
        // Live only: a video job's progress is worth showing and worth forgetting (05 §2).
        StreamEvent::Notice { kind, detail } => {
            batcher.push(AgentEventKind::ProviderNotice { kind, detail });
        }
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
            let entry = ctx
                .tools
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .resolve(&name)
                .cloned();
            let (connector, tool) = match &entry {
                Some(e) => (e.connector_id().to_owned(), e.def.name.clone()),
                None => ToolSet::split_name(&name),
            };
            let connector_name = entry
                .as_ref()
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
                tier: entry.as_ref().map_or(RiskTier::Read, |e| e.def.tier),
                status: ToolCallStatus::Proposed,
                decision_source: None,
                judge: None,
                display: display_for(entry.as_ref().map(|e| &e.def), &serde_json::Value::Null),
                result_preview: None,
                result: None,
                result_blob_hash: None,
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
                let entry = ctx
                    .tools
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .resolve(name)
                    .cloned();
                let tier = entry.as_ref().map_or(RiskTier::Read, |e| e.def.tier);
                let display = display_for(entry.as_ref().map(|e| &e.def), &args);
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
    complete_decided(ctx, id, status, None, is_error, duration_ms, result)
}

/// The same, for a call that never ran, naming who refused it. A call that *did* run said so in
/// `tool_call.executing`; one that did not has nowhere else to record the decision, and "what
/// ran, and who allowed it" has to stay a single query (04 §11).
fn complete_decided(
    ctx: &RunContext,
    id: &CallId,
    status: ToolCallStatus,
    decision_source: Option<DecisionSource>,
    is_error: bool,
    duration_ms: u64,
    result: Vec<ResultPart>,
) -> ContentPart {
    let preview = result_preview(&result, PREVIEW_CHARS);
    let mut output_blob = None;
    update_call(ctx, id, |c| {
        output_blob = c.result_blob_hash.clone();
        if let Some(source) = decision_source {
            c.decision_source = Some(source);
        }
        c.status = status;
        c.is_error = is_error;
        c.result_preview = Some(preview.clone());
        c.result = Some(result.clone());
        c.ended_at = Some(now_ms());
        c.duration_ms = Some(duration_ms);
    });
    {
        // The tail existed only for a view that reattached while the call ran; from here the
        // result carries the output, and keeping both would send it twice.
        let mut state = ctx.active.state.lock().unwrap_or_else(|e| e.into_inner());
        state.output.remove(id);
    }
    ctx.active.batcher.push(AgentEventKind::ToolCallCompleted {
        call_id: id.clone(),
        status,
        decision_source,
        is_error,
        duration_ms,
        result_preview: preview,
        result: result.clone(),
        output_blob,
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

/// Steps 4 and 5 of 01 §3 for one batch of calls: decide each, execute the allowed ones, and
/// return one result per call in the model's order plus whether the user cancelled meanwhile.
///
/// Deciding happens in three passes, because the three kinds of answer take different amounts
/// of time and the user should never wait for one they did not need. The rules answer first and
/// instantly; the guard's questions all go out at once, so a batch of four costs one round trip
/// rather than four; and only then are the prompts raised, in the model's own order, so the
/// cards stack the way the calls were made.
async fn run_calls(
    ctx: &RunContext,
    assistant: &Message,
    calls: &[Call],
    guard: &mut GuardState,
) -> (Vec<ContentPart>, bool) {
    let batcher = ctx.active.batcher.clone();
    let mut results: Vec<Option<ContentPart>> = (0..calls.len()).map(|_| None).collect();
    // Index, tool, who allowed it, and whether the attaching a `widens_access` call exists to
    // ask about has already been answered (04 §9) — by the mode, by the guard, or by **Allow
    // anyway**. Only `Decision::Ask`, which is the user being asked, leaves it unanswered.
    let mut allowed: Vec<(usize, ToolEntry, DecisionSource, bool)> = Vec::new();
    let mut judged: Vec<(usize, ToolEntry)> = Vec::new();
    let mut asking: Vec<(
        usize,
        ToolEntry,
        Option<gantry_core::GuardrailHit>,
        Option<String>,
    )> = Vec::new();
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
        let Some(entry) = ctx
            .tools
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .resolve(&call.name)
            .cloned()
        else {
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
            &ctx.guardrails,
        );
        match decision {
            // Every mode allows an `App` call on the `Mode` source; only unguarded Auto means
            // nobody else was ever going to look at it.
            Decision::Allow(source) => {
                let decided = source == DecisionSource::Mode
                    && ctx.input.mode == gantry_core::Mode::Auto
                    && !ctx.input.guard;
                allowed.push((i, entry, source, decided));
            }
            Decision::Deny { source, reason } => {
                guard.denials += 1;
                guard.recent.push(recent_of(call, "refused", "by a rule"));
                results[i] = Some(complete_decided(
                    ctx,
                    &call.id,
                    ToolCallStatus::Denied,
                    Some(source),
                    true,
                    0,
                    vec![ResultPart::Json {
                        json: json!({ "error": "denied_by_policy", "message": reason }),
                    }],
                ));
            }
            // The user already answered this one, by hand, after the guard blocked it.
            Decision::Judge
                if ctx
                    .overrides
                    .take(ctx.input.chat_id, &call.name, &call.args) =>
            {
                // **Allow anyway** is the user answering this exact question by hand; asking
                // them again on the tool's own card would be asking twice for one click.
                allowed.push((i, entry, DecisionSource::UserOnce, true));
            }
            Decision::Judge => judged.push((i, entry)),
            Decision::Ask { guardrail } => asking.push((i, entry, guardrail, None)),
        }
    }

    // The guard's questions, all at once (04 §6). A verdict either settles the call or, when
    // the guard could not reach one, hands it to the user with the reason why.
    if !judged.is_empty() {
        let frame = guard_frame(ctx, assistant);
        let mut verdicts = Vec::with_capacity(judged.len());
        for chunk in judged.chunks(judge::MAX_IN_FLIGHT) {
            verdicts.extend(
                futures_util::future::join_all(
                    chunk
                        .iter()
                        .map(|(i, entry)| ask_the_guard(ctx, &calls[*i], entry, &frame, guard)),
                )
                .await,
            );
        }
        for ((i, entry), outcome) in judged.into_iter().zip(verdicts) {
            let call = &calls[i];
            match outcome {
                Ok(verdict) => {
                    batcher.push(AgentEventKind::JudgeDecision {
                        call_id: call.id.clone(),
                        verdict: Box::new(verdict.clone()),
                    });
                    update_call(ctx, &call.id, |c| c.judge = Some(verdict.clone()));
                    if judge::needs_the_user(&verdict, entry.def.tier) {
                        asking.push((
                            i,
                            entry,
                            None,
                            Some(format!(
                                "The guard was not sure enough to allow something irreversible: {}",
                                verdict.reason
                            )),
                        ));
                    } else if verdict.allows() {
                        allowed.push((i, entry, DecisionSource::Judge, true));
                    } else {
                        guard.denials += 1;
                        guard
                            .recent
                            .push(recent_of(call, "blocked", "by the guard"));
                        results[i] = Some(complete_decided(
                            ctx,
                            &call.id,
                            ToolCallStatus::Denied,
                            Some(DecisionSource::Judge),
                            true,
                            0,
                            vec![ResultPart::Json {
                                json: verdict.blocked_result(),
                            }],
                        ));
                    }
                }
                // Fail closed (04 §1): a guard that cannot decide is not an allow.
                Err(err) => {
                    let detail = err.to_string();
                    log::warn!("the guard could not decide about {}: {detail}", call.name);
                    batcher.push(AgentEventKind::ProviderNotice {
                        kind: "guard_unavailable".into(),
                        detail: format!("{detail}; asking you instead."),
                    });
                    batcher.push(AgentEventKind::JudgeDecision {
                        call_id: call.id.clone(),
                        verdict: Box::new(JudgeVerdict::from_rule(
                            JudgeDecision::Deny,
                            JudgeSource::Unavailable,
                            &detail,
                            Vec::new(),
                        )),
                    });
                    // A call whose own tool asks the user is already the fallback (04 §9).
                    // Putting a permission card in front of it asks the same question twice —
                    // once about the call, once about what the call is for — so it runs, and
                    // the card it raises for itself is the question.
                    if entry.def.widens_access {
                        allowed.push((i, entry, DecisionSource::Mode, false));
                    } else {
                        asking.push((
                            i,
                            entry,
                            None,
                            Some(format!("{detail}, so this one is yours.")),
                        ));
                    }
                }
            }
        }
    }

    // The cards, in the model's order however the answers arrived at them.
    asking.sort_by_key(|(i, _, _, _)| *i);
    for (i, entry, guardrail, guard_note) in asking {
        let call = &calls[i];
        let guardrail_kind = guardrail.as_ref().map(|g| g.kind);
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
                    guardrail,
                    guard: guard_note,
                    // Only the scopes the engine would honour on a later call: a guardrail
                    // is never answered by a grant (04 §5), and `always_confirm` asks by
                    // definition, so offering "for this chat" on either would write a grant
                    // that changes nothing and ask again next turn.
                    scopes: GrantScope::for_call(
                        entry.def.tier,
                        &call.args,
                        guardrail_kind,
                        entry.def.always_confirm,
                    ),
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
        match &resolution {
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
                allowed.push((i, entry, source, false));
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
            // Everything the guard above did not take is a denial. A resolution of another
            // kind cannot reach a permission card, and is refused the same way if it does.
            other => {
                let message = match other {
                    InteractionResolution::Permission { message, .. } => message.clone(),
                    _ => None,
                };
                batcher.push(AgentEventKind::DecisionResolved {
                    interaction_id: interaction.id,
                    resolution: InteractionResolution::Permission {
                        decision: PermissionDecision::Deny,
                        message: message.clone(),
                    },
                    source: DecisionSource::UserOnce,
                });
                results[i] = Some(complete_decided(
                    ctx,
                    &call.id,
                    ToolCallStatus::Denied,
                    Some(DecisionSource::UserOnce),
                    true,
                    0,
                    denied_result(message),
                ));
            }
        }
        notify_pending(ctx);
    }

    if cancelled {
        for (i, _, _, _) in allowed {
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
        let parallel = allowed.iter().all(|(_, e, _, _)| e.def.parallel_safe);
        for (i, entry, source, _) in &allowed {
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
                .map(|(i, entry, _, decided)| execute(ctx, &calls[*i], entry.clone(), *decided));
            let outcomes = futures_util::future::join_all(futures).await;
            for ((i, _, _, _), part) in allowed.iter().zip(outcomes) {
                results[*i] = Some(part);
            }
        } else {
            for (i, entry, _, decided) in &allowed {
                results[*i] = Some(execute(ctx, &calls[*i], entry.clone(), *decided).await);
            }
        }
        cancelled = ctx.active.cancel.is_cancelled();
        // What the guard is told about this batch when it decides the next one. A failure is
        // also a strike against the call: three of the same and the loop detector answers
        // without asking anyone (04 §6).
        for (i, _, source, _) in &allowed {
            let call = &calls[*i];
            let failed = matches!(
                &results[*i],
                Some(ContentPart::ToolResult { is_error: true, .. })
            );
            if failed {
                guard.loops.failed(&call.name, &call.args);
            }
            guard.recent.push(recent_of(
                call,
                if failed { "failed" } else { "ok" },
                match source {
                    DecisionSource::Judge => "allowed by the guard",
                    DecisionSource::UserOnce | DecisionSource::UserChatGrant => {
                        "allowed by the user"
                    }
                    _ => "allowed by the mode",
                },
            ));
        }
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

/// The task as the guard sees it (04 §6): what the user asked for, where the work may happen,
/// and what the assistant said it was about to do. Built once per batch, because every call in
/// a batch shares it.
fn guard_frame(ctx: &RunContext, assistant: &Message) -> judge::Frame {
    let user_texts: Vec<String> = ctx
        .input
        .messages
        .iter()
        .filter(|m| m.role == Role::User)
        .map(|m| m.text())
        .filter(|t| !t.trim().is_empty())
        .collect();
    judge::Frame {
        // Projects arrive with M11; until then a chat belongs to nothing and the guard is told
        // so rather than told a name that would be a guess.
        project: None,
        first_user: user_texts.first().cloned().unwrap_or_default(),
        last_user: user_texts.last().cloned().unwrap_or_default(),
        intent: last_sentence(&assistant.text()),
        roots: ctx.input.roots.clone(),
        mode: ctx.input.mode,
    }
}

/// One call put to the guard, with the loop detector in front of it (04 §6, rule 4). The loop
/// is answered here rather than by the model because the answer cannot depend on judgement: the
/// same call has already failed three times, and asking a model about it a fourth time costs
/// money to be told what we know.
async fn ask_the_guard(
    ctx: &RunContext,
    call: &Call,
    entry: &ToolEntry,
    frame: &judge::Frame,
    guard: &GuardState,
) -> Result<JudgeVerdict, judge::JudgeError> {
    if let Some(verdict) = guard.loops.verdict(&call.name, &call.args) {
        return Ok(verdict);
    }
    let Some((provider, model)) = ctx.judge.clone() else {
        return Err(judge::JudgeError::Unavailable);
    };
    let action = judge::Action {
        connector: entry.connector_name().to_owned(),
        tool: entry.def.name.clone(),
        description: entry.def.description.clone(),
        tier: entry.def.tier,
        args: call.args.clone(),
        // A dry run of the change is not available yet: it needs a connector that can compute
        // one without performing it, which no connector offers (04 §6, recorded as a gap).
        preview: None,
    };
    let input = judge::render(frame, &action, &guard.recent, guard.denials);
    judge::decide(provider, model, input).await
}

/// One line of the turn's history, as the guard reads it back on the next call.
fn recent_of(call: &Call, outcome: &'static str, decision: &str) -> judge::Recent {
    judge::Recent {
        tool: call.name.clone(),
        args: display_for(None, &call.args).summary,
        outcome,
        decision: decision.to_owned(),
    }
}

/// Runs one allowed call to its result part; cancellation yields an error result.
async fn execute(
    ctx: &RunContext,
    call: &Call,
    entry: ToolEntry,
    // 04 §9: whether the attaching this call exists to ask about already has its answer.
    attach_decided: bool,
) -> ContentPart {
    let started = Instant::now();
    let req = ToolCallRequest {
        call_id: call.id.clone(),
        tool: entry.def.name.clone(),
        args: call.args.clone(),
        scope: ChatScope {
            chat_id: ctx.input.chat_id,
            turn_id: ctx.input.turn_id,
            mode: ctx.input.mode,
            attach_decided,
        },
    };
    let cancel = ctx.active.cancel.child_token();
    let sink = Arc::new(TurnToolEvents {
        batcher: ctx.active.batcher.clone(),
        active: ctx.active.clone(),
        interactions: ctx.interactions.clone(),
        notifier: ctx.notifier.clone(),
        chat_id: ctx.input.chat_id,
        turn_id: ctx.input.turn_id,
        cancel: cancel.clone(),
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
        })) => {
            let (capped, whole) = cap_result(content, ctx.max_result_bytes);
            if let Some(text) = whole {
                keep_whole_output(ctx, &call.id, &text);
            }
            (ToolCallStatus::Completed, is_error, capped)
        }
    };
    complete_call(ctx, &call.id, status, is_error, elapsed, content)
}

/// Stores the output a cap cut down, so the row's drawer can show what the model was not sent
/// (05 §8, 06 §3). Called before the call is completed, so the hash rides on the completion
/// event and reaches the row in the same write as the rest of it.
///
/// A failure is logged and nothing else: losing the copy is not a reason to fail a tool call
/// that has already succeeded.
fn keep_whole_output(ctx: &RunContext, id: &CallId, text: &str) {
    match ctx.chats.blobs().put(text.as_bytes()) {
        Ok(hash) => update_call(ctx, id, |c| c.result_blob_hash = Some(hash)),
        Err(err) => log::warn!("could not keep the whole output of {id}: {err}"),
    }
}

/// Rebuilds the tool set when the chat's connectors changed while the turn was running: an
/// access request the user granted (04 §9), a suggestion they installed (03 §9), or the `+`
/// menu. Returns the `System` message that tells the model, so it never calls a tool it has
/// just lost or misses one it has just been given.
async fn refresh_tools(ctx: &RunContext, attached: &mut Vec<String>) -> Option<Message> {
    let now = match ctx.chats.attached_connectors(ctx.input.chat_id) {
        Ok(list) => list,
        Err(err) => {
            log::warn!("could not re-read the chat's connectors: {err}");
            return None;
        }
    };
    if now == *attached {
        return None;
    }
    let added: Vec<String> = now
        .iter()
        .filter(|n| !attached.contains(n))
        .cloned()
        .collect();
    let removed: Vec<String> = attached
        .iter()
        .filter(|n| !now.contains(n))
        .cloned()
        .collect();
    *attached = now.clone();
    let set = ToolSet::assemble(&ctx.connectors, ctx.input.mode, &now, !ctx.input.incognito).await;
    *ctx.tools.write().unwrap_or_else(|e| e.into_inner()) = set;
    Some(Message {
        id: MessageId::new(),
        role: Role::System,
        parts: vec![ContentPart::ToolSetChange { added, removed }],
        origin: None,
        created_at: now_ms(),
    })
}

/// The sink a running call reports through: events a runtime tool produces itself
/// (`artifact.*`) join the turn stream; output and progress arrive with the shell (M7).
struct TurnToolEvents {
    batcher: Arc<crate::events::Batcher>,
    active: Arc<ActiveTurn>,
    interactions: Arc<Interactions>,
    notifier: Option<Arc<dyn ChatNotifier>>,
    chat_id: gantry_core::ChatId,
    /// An elicitation is raised against the turn, and answered `cancel` when the turn stops.
    turn_id: gantry_core::TurnId,
    cancel: CancellationToken,
}

impl TurnToolEvents {
    fn pending_changed(&self) {
        if let Some(n) = &self.notifier {
            n.interactions_changed(self.chat_id, self.interactions.pending_count(self.chat_id));
        }
    }
}

#[async_trait::async_trait]
impl ToolEventSink for TurnToolEvents {
    /// A running call's output, on its way to the feed. Transient by design (05 §2): the end
    /// state is the call's result, so nothing here is persisted or replayed.
    ///
    /// A bounded tail is kept on the turn all the same, and only while the call runs. Transient
    /// means "not in the transcript", not "unavailable to a view that arrives late": without
    /// it, reattaching in the middle of a two-minute build showed a row with no output at all
    /// until the command finished.
    fn output(&self, call_id: &gantry_core::CallId, stream: OutputStream, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let chunk = String::from_utf8_lossy(chunk).into_owned();
        {
            let mut s = self.active.state.lock().unwrap_or_else(|e| e.into_inner());
            s.push_output(call_id, &chunk);
        }
        self.batcher.push(AgentEventKind::ToolCallOutput {
            call_id: call_id.clone(),
            stream,
            chunk,
        });
    }

    fn event(&self, event: AgentEventKind) {
        // A tool that asks the user itself (03 §9, 04 §9) raises its card through this sink.
        // The turn's own pending list and the sidebar badge follow it, so a reattached view
        // and the chat list see it exactly as they see a permission prompt.
        match &event {
            AgentEventKind::DecisionRequested { interaction } => {
                {
                    let mut s = self.active.state.lock().unwrap_or_else(|e| e.into_inner());
                    if !s.pending.iter().any(|p| p.id == interaction.id) {
                        s.pending.push((**interaction).clone());
                    }
                }
                self.pending_changed();
            }
            AgentEventKind::DecisionResolved { interaction_id, .. } => {
                {
                    let mut s = self.active.state.lock().unwrap_or_else(|e| e.into_inner());
                    s.pending.retain(|p| p.id != *interaction_id);
                }
                self.pending_changed();
            }
            _ => {}
        }
        self.batcher.push(event);
    }

    /// A server stopping mid-call to ask the user something (03 §6): the same card machinery as
    /// a permission prompt, because to the person answering it is the same kind of moment — the
    /// turn has stopped and is waiting on them.
    ///
    /// Cancelling the turn answers it `cancel`, which is the specification's own word for "the
    /// user is not going to answer this". A server that gets it can stop rather than wait.
    async fn elicit(&self, request: gantry_core::ElicitationRequest) -> ElicitationAnswer {
        let interaction = Interaction::pending(
            self.chat_id,
            self.turn_id,
            InteractionPayload::Elicitation { request },
        );
        let id = interaction.id;
        let rx = self.interactions.request(interaction.clone());
        self.event(AgentEventKind::DecisionRequested {
            interaction: Box::new(interaction),
        });
        let resolution = tokio::select! {
            () = self.cancel.cancelled() => InteractionResolution::Cancelled,
            r = rx => r.unwrap_or(InteractionResolution::Cancelled),
        };
        self.event(AgentEventKind::DecisionResolved {
            interaction_id: id,
            resolution: resolution.clone(),
            source: DecisionSource::UserOnce,
        });
        match resolution {
            InteractionResolution::Elicitation { action, values } => {
                ElicitationAnswer { action, values }
            }
            _ => ElicitationAnswer {
                action: gantry_core::ElicitationAction::Cancel,
                values: serde_json::Value::Object(serde_json::Map::new()),
            },
        }
    }
}

/// Keeps a result under the transcript limit: head and tail with a marker between (05 §8).
/// Returns the capped parts, and the whole output as text when something was cut.
///
/// The capping happens at ingestion, so the capped form is what is written down and what every
/// later request carries. That is deliberate: a cap applied at projection time would make the
/// same message mean different things on different turns, and 02 §6 forbids history that
/// changes under the model. What the middle is cut *out of* is kept as a blob, which is where
/// the row's drawer reads it from — until M13 it was simply lost, and a comment here said
/// otherwise.
fn cap_result(content: Vec<ResultPart>, max: usize) -> (Vec<ResultPart>, Option<String>) {
    let size: usize = content
        .iter()
        .map(|p| match p {
            ResultPart::Text { text } => text.len(),
            ResultPart::Json { json } => json.to_string().len(),
            ResultPart::Image { data, .. } => data.len(),
            ResultPart::Resource { summary, .. } => summary.len(),
        })
        .sum();
    if size <= max {
        return (content, None);
    }
    let text = result_preview(&content, usize::MAX);
    let half = (max / 2).max(1);
    let head: String = text.chars().take(half).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(half)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let capped = vec![ResultPart::Text {
        text: format!(
            "{head}\n\n[… {} characters omitted …]\n\n{tail}",
            text.chars().count().saturating_sub(2 * half)
        ),
    }];
    (capped, Some(text))
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
    fn oversized_results_keep_head_and_tail_and_hand_back_the_whole_thing() {
        let big = "x".repeat(RESULT_MAX_BYTES + 100);
        let (capped, full) = cap_result(
            vec![ResultPart::Text { text: big.clone() }],
            RESULT_MAX_BYTES,
        );
        let ResultPart::Text { text } = &capped[0] else {
            panic!()
        };
        assert!(text.contains("characters omitted"));
        assert!(text.len() < RESULT_MAX_BYTES + 100);
        assert_eq!(full.as_deref(), Some(big.as_str()), "nothing is lost");

        let (small, full) = cap_result(
            vec![ResultPart::Text {
                text: "short".into(),
            }],
            RESULT_MAX_BYTES,
        );
        assert_eq!(small.len(), 1);
        assert!(
            full.is_none(),
            "nothing was cut, so there is no blob to keep"
        );
    }
}
