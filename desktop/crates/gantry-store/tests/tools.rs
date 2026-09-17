//! Migration 0003: tool calls, interactions, the projection and crash recovery.

use gantry_core::{
    AgentEvent, AgentEventKind, CallId, ChatId, ContentPart, DecisionSource, Interaction,
    InteractionPayload, InteractionResolution, InteractionStatus, Message, MessageId, Mode,
    ModelRef, PermissionDecision, PermissionRequest, ReasoningEffort, ResultPart, RiskTier, Role,
    ToolCallStatus, ToolDisplay, ToolDisplayKind, TurnId, TurnStatus, now_ms,
};
use gantry_store::{
    Store,
    repos::{chats, interactions, messages, projections, recovery, tool_calls, turns},
};

fn open() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("t.db")).unwrap();
    (dir, store)
}

fn seed(conn: &rusqlite::Connection) -> (ChatId, TurnId) {
    let now = now_ms();
    let chat = chats::ChatRecord {
        surface: gantry_core::Surface::Chat,
        id: ChatId::new(),
        project_id: None,
        title: "t".into(),
        title_source: "auto".into(),
        pinned: false,
        mode: Mode::Manual,
        guard: true,
        model: ModelRef::default_model(),
        effort: ReasoningEffort::Off,
        web_search: false,
        instructions: String::new(),
        system_snapshot: "sys".into(),
        system_snapshot_version: 1,
        created_at: now,
        updated_at: now,
        last_message_at: now,
        archived_at: None,
        incognito: false,
        parent_turn_id: None,
        agent_type: None,
    };
    chats::insert(conn, &chat).unwrap();
    let turn = turns::TurnRecord {
        id: TurnId::new(),
        chat_id: chat.id,
        seq: 1,
        status: TurnStatus::Running,
        model: ModelRef::default_model(),
        started_at: now,
        ended_at: None,
        usage: None,
        stop_reason: None,
        error: None,
        feedback: None,
        tool_call_count: 0,
    };
    turns::insert(conn, &turn).unwrap();
    (chat.id, turn.id)
}

fn ev(turn_id: TurnId, seq: u32, event: AgentEventKind) -> AgentEvent {
    AgentEvent {
        seq,
        ts: now_ms(),
        turn_id,
        event,
    }
}

#[test]
fn the_projection_follows_a_call_from_start_to_completion() {
    let (_dir, store) = open();
    store
        .write_blocking(|conn| {
            let (chat_id, turn_id) = seed(conn);
            let call_id = CallId::new();
            let message_id = MessageId::new();
            let events = vec![
                AgentEventKind::ToolCallStarted {
                    call_id: call_id.clone(),
                    message_id,
                    connector: "gantry".into(),
                    connector_name: "Gantry".into(),
                    tool: "clock".into(),
                    model_tool_name: "gantry__clock".into(),
                },
                AgentEventKind::ToolCallReady {
                    call_id: call_id.clone(),
                    args: serde_json::json!({}),
                    tier: RiskTier::Read,
                    display: ToolDisplay {
                        kind: ToolDisplayKind::Connector,
                        summary: "now".into(),
                    },
                },
            ];
            for (i, e) in events.into_iter().enumerate() {
                projections::apply(conn, chat_id, &ev(turn_id, i as u32 + 1, e))?;
            }
            let c = tool_calls::get(conn, &call_id)?.unwrap();
            assert_eq!(c.status, ToolCallStatus::Proposed);
            assert_eq!(c.display.summary, "now");

            let interaction = Interaction::pending(
                chat_id,
                turn_id,
                InteractionPayload::Permission {
                    request: Box::new(PermissionRequest {
                        call_id: call_id.clone(),
                        connector: "gantry".into(),
                        connector_name: "Gantry".into(),
                        tool: "clock".into(),
                        model_tool_name: "gantry__clock".into(),
                        tier: RiskTier::Read,
                        args: serde_json::json!({}),
                        display: c.display.clone(),
                        why: None,
                        description: String::new(),
                        guardrail: None,
                        guard: None,
                        scopes: Vec::new(),
                        choices: Vec::new(),
                    }),
                },
            );
            projections::apply(
                conn,
                chat_id,
                &ev(
                    turn_id,
                    3,
                    AgentEventKind::DecisionRequested {
                        interaction: Box::new(interaction.clone()),
                    },
                ),
            )?;
            assert_eq!(
                tool_calls::get(conn, &call_id)?.unwrap().status,
                ToolCallStatus::AwaitingDecision
            );
            assert_eq!(interactions::list_pending(conn, Some(chat_id))?.len(), 1);
            assert_eq!(interactions::pending_counts(conn)?, vec![(chat_id, 1)]);

            projections::apply(
                conn,
                chat_id,
                &ev(
                    turn_id,
                    4,
                    AgentEventKind::DecisionResolved {
                        interaction_id: interaction.id,
                        resolution: InteractionResolution::Permission {
                            decision: PermissionDecision::AllowOnce,
                            message: None,
                            chosen: Default::default(),
                        },
                        source: DecisionSource::UserOnce,
                    },
                ),
            )?;
            let stored = interactions::get(conn, interaction.id)?.unwrap();
            assert_eq!(stored.status, InteractionStatus::Resolved);
            assert!(interactions::list_pending(conn, None)?.is_empty());

            projections::apply(
                conn,
                chat_id,
                &ev(
                    turn_id,
                    5,
                    AgentEventKind::ToolCallExecuting {
                        call_id: call_id.clone(),
                        source: DecisionSource::UserOnce,
                    },
                ),
            )?;
            projections::apply(
                conn,
                chat_id,
                &ev(
                    turn_id,
                    6,
                    AgentEventKind::ToolCallCompleted {
                        call_id: call_id.clone(),
                        status: ToolCallStatus::Completed,
                        decision_source: None,
                        is_error: false,
                        duration_ms: 3,
                        result_preview: "2026".into(),
                        result: vec![ResultPart::Text {
                            text: "2026".into(),
                        }],
                        output_blob: Some("a".repeat(64)),
                    },
                ),
            )?;
            let c = tool_calls::get(conn, &call_id)?.unwrap();
            assert_eq!(c.status, ToolCallStatus::Completed);
            assert_eq!(c.decision_source, Some(DecisionSource::UserOnce));
            assert_eq!(c.result_preview.as_deref(), Some("2026"));
            assert_eq!(
                c.result_blob_hash,
                Some("a".repeat(64)),
                "the whole output's blob reaches the row"
            );
            assert_eq!(c.duration_ms, Some(3));
            assert!(c.started_at.is_some() && c.ended_at.is_some());
            assert_eq!(tool_calls::list_for_turn(conn, turn_id)?.len(), 1);
            Ok(())
        })
        .unwrap();
}

#[test]
fn recovery_closes_calls_prompts_and_transcripts() {
    let (_dir, store) = open();
    store
        .write_blocking(|conn| {
            let (chat_id, turn_id) = seed(conn);
            let call_id = CallId::new();
            let message_id = MessageId::new();
            projections::apply(
                conn,
                chat_id,
                &ev(
                    turn_id,
                    1,
                    AgentEventKind::ToolCallStarted {
                        call_id: call_id.clone(),
                        message_id,
                        connector: "gantry".into(),
                        connector_name: "Gantry".into(),
                        tool: "clock".into(),
                        model_tool_name: "gantry__clock".into(),
                    },
                ),
            )?;
            let interaction = Interaction::pending(
                chat_id,
                turn_id,
                InteractionPayload::Permission {
                    request: Box::new(PermissionRequest {
                        call_id: call_id.clone(),
                        connector: "gantry".into(),
                        connector_name: "Gantry".into(),
                        tool: "clock".into(),
                        model_tool_name: "gantry__clock".into(),
                        tier: RiskTier::Read,
                        args: serde_json::json!({}),
                        display: ToolDisplay {
                            kind: ToolDisplayKind::Connector,
                            summary: String::new(),
                        },
                        why: None,
                        description: String::new(),
                        guardrail: None,
                        guard: None,
                        scopes: Vec::new(),
                        choices: Vec::new(),
                    }),
                },
            );
            interactions::insert(conn, &interaction)?;
            // The assistant message with the call was persisted; its result never came.
            messages::insert(
                conn,
                &messages::MessageRecord {
                    message: Message {
                        id: message_id,
                        role: Role::Assistant,
                        parts: vec![ContentPart::ToolCall {
                            signature: None,
                            id: call_id.clone(),
                            name: "gantry__clock".into(),
                            args: serde_json::json!({}),
                        }],
                        origin: None,
                        created_at: now_ms(),
                    },
                    chat_id,
                    turn_id: Some(turn_id),
                    seq: 1,
                    stop_reason: None,
                    usage: None,
                },
            )?;

            let r = recovery::run(conn, now_ms())?;
            assert_eq!(
                r,
                recovery::Recovered {
                    turns: 1,
                    tool_calls: 1,
                    interactions: 1,
                    results: 1
                }
            );
            assert_eq!(
                turns::get(conn, turn_id)?.unwrap().status,
                TurnStatus::Interrupted
            );
            assert_eq!(
                tool_calls::get(conn, &call_id)?.unwrap().status,
                ToolCallStatus::Cancelled
            );
            assert_eq!(
                interactions::get(conn, interaction.id)?.unwrap().status,
                InteractionStatus::Cancelled
            );
            let rows = messages::list_for_turn(conn, turn_id)?;
            assert_eq!(rows.len(), 2);
            assert!(matches!(
                &rows[1].message.parts[0],
                ContentPart::ToolResult { call_id: c, is_error: true, .. } if *c == call_id
            ));
            // A second sweep finds nothing.
            assert!(recovery::run(conn, now_ms())?.is_empty());
            Ok(())
        })
        .unwrap();
}
