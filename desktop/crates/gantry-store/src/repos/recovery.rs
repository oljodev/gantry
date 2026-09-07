//! Crash recovery at startup (docs/plan/01 §3, 09 M2): whatever the previous process left
//! half-done is closed so every chat can continue and every transcript can be replayed.

use gantry_core::{CallId, ContentPart, Message, MessageId, ResultPart, Role, TurnId, TurnStatus};
use rusqlite::Connection;

use crate::{
    db::Result,
    repos::{interactions, messages, tool_calls, turns},
};

/// What the sweep found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Recovered {
    pub turns: usize,
    pub tool_calls: usize,
    pub interactions: usize,
    /// Tool calls that got a synthetic error result so the transcript stays complete.
    pub results: usize,
}

impl Recovered {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

pub const INTERRUPTED_RESULT: &str =
    "Gantry was closed before this tool call finished; it did not run to completion.";

pub fn run(conn: &Connection, now: i64) -> Result<Recovered> {
    let running: Vec<TurnId> = turns::chats_with_running_turns(conn)?
        .into_iter()
        .map(|(_, t)| t)
        .collect();
    let mut out = Recovered {
        turns: turns::interrupt_running(conn, now)?,
        tool_calls: tool_calls::cancel_open(conn, now)?,
        interactions: interactions::cancel_pending(conn, now)?,
        results: 0,
    };
    for turn in running {
        out.results += synthesize_missing_results(conn, turn, now)?;
    }
    Ok(out)
}

/// Appends a `Tool` message with an error result for every `ToolCall` of the turn that has no
/// `ToolResult` (02 §3: a transcript is never sent with a call lacking a result).
pub fn synthesize_missing_results(conn: &Connection, turn_id: TurnId, now: i64) -> Result<usize> {
    let Some(turn) = turns::get(conn, turn_id)? else {
        return Ok(0);
    };
    if turn.status == TurnStatus::Running {
        return Ok(0);
    }
    let rows = messages::list_for_turn(conn, turn_id)?;
    let mut open: Vec<CallId> = Vec::new();
    for m in &rows {
        for p in &m.message.parts {
            match p {
                ContentPart::ToolCall { id, .. } => open.push(id.clone()),
                ContentPart::ToolResult { call_id, .. } => open.retain(|c| c != call_id),
                _ => {}
            }
        }
    }
    if open.is_empty() {
        return Ok(0);
    }
    let count = open.len();
    let parts = open
        .into_iter()
        .map(|call_id| ContentPart::ToolResult {
            call_id,
            content: vec![ResultPart::Text {
                text: INTERRUPTED_RESULT.to_owned(),
            }],
            is_error: true,
        })
        .collect();
    messages::insert(
        conn,
        &messages::MessageRecord {
            message: Message {
                id: MessageId::new(),
                role: Role::Tool,
                parts,
                origin: None,
                created_at: now,
            },
            chat_id: turn.chat_id,
            turn_id: Some(turn_id),
            seq: messages::next_seq(conn, turn.chat_id)?,
            stop_reason: None,
            usage: None,
        },
    )?;
    Ok(count)
}
