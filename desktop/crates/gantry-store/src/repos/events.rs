//! The `events` table: the persisted subset of agent events, the append-only activity log
//! (docs/plan/05 §2, 06 §3).

use gantry_core::{AgentEvent, ChatId, EventId, TurnId};
use rusqlite::{Connection, params};

use crate::{db::Result, repos::id_from_str};

#[derive(Debug, Clone, PartialEq)]
pub struct EventRecord {
    pub id: EventId,
    pub chat_id: ChatId,
    pub event: AgentEvent,
    /// A call id, interaction id or edit id the event refers to.
    pub ref_id: Option<String>,
}

/// Writes a batch in one transaction.
pub fn insert_batch(conn: &mut Connection, events: &[EventRecord]) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO events (id, chat_id, turn_id, seq, ts, kind, ref_id, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for e in events {
            let payload = serde_json::to_value(&e.event.event)
                .map_err(|err| crate::StoreError::Other(err.to_string()))?;
            let kind = payload
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown")
                .to_owned();
            stmt.execute(params![
                e.id.to_string(),
                e.chat_id.to_string(),
                e.event.turn_id.to_string(),
                e.event.seq,
                e.event.ts,
                kind,
                e.ref_id,
                payload.to_string(),
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// The persisted events of a turn in sequence order.
pub fn list_for_turn(conn: &Connection, turn_id: TurnId) -> Result<Vec<AgentEvent>> {
    let mut stmt = conn.prepare(
        "SELECT turn_id, seq, ts, payload_json FROM events WHERE turn_id = ?1 ORDER BY seq",
    )?;
    let rows = stmt.query_map(params![turn_id.to_string()], |r| {
        let payload: String = r.get(3)?;
        let event = serde_json::from_str(&payload).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
        })?;
        Ok(AgentEvent {
            turn_id: id_from_str(r, 0)?,
            seq: r.get(1)?,
            ts: r.get(2)?,
            event,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// `(turn, detail)` of every `provider.notice` in the chat, in order, for the turn DTOs.
pub fn list_notices_for_chat(conn: &Connection, chat_id: ChatId) -> Result<Vec<(TurnId, String)>> {
    let mut stmt = conn.prepare(
        "SELECT turn_id, payload_json FROM events
         WHERE chat_id = ?1 AND kind = 'provider.notice' ORDER BY turn_id, seq",
    )?;
    let rows = stmt.query_map(params![chat_id.to_string()], |r| {
        let payload: String = r.get(1)?;
        let detail = serde_json::from_str::<serde_json::Value>(&payload)
            .ok()
            .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(str::to_owned))
            .unwrap_or_default();
        Ok((id_from_str::<TurnId>(r, 0)?, detail))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn count_for_turn(conn: &Connection, turn_id: TurnId) -> Result<u32> {
    Ok(conn.query_row(
        "SELECT count(*) FROM events WHERE turn_id = ?1",
        params![turn_id.to_string()],
        |r| r.get(0),
    )?)
}
