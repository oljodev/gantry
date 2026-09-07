//! The `interactions` table (docs/plan/04 §10, 06 §3): pending decisions survive navigation;
//! a restart cancels them.

use gantry_core::{
    ChatId, Interaction, InteractionId, InteractionKind, InteractionPayload, InteractionResolution,
    InteractionStatus, TurnId,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    db::Result,
    repos::{enum_from_str, enum_to_str, id_from_str},
};

const COLUMNS: &str =
    "id, chat_id, turn_id, kind, payload_json, status, resolution_json, created_at, resolved_at";

fn conv(idx: usize, e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<Interaction> {
    let payload: String = r.get(4)?;
    let resolution: Option<String> = r.get(6)?;
    Ok(Interaction {
        id: id_from_str(r, 0)?,
        chat_id: id_from_str(r, 1)?,
        turn_id: id_from_str(r, 2)?,
        kind: enum_from_str::<InteractionKind>(r, 3)?,
        payload: serde_json::from_str::<InteractionPayload>(&payload).map_err(|e| conv(4, e))?,
        status: enum_from_str::<InteractionStatus>(r, 5)?,
        resolution: resolution
            .map(|s| serde_json::from_str::<InteractionResolution>(&s))
            .transpose()
            .map_err(|e| conv(6, e))?,
        created_at: r.get(7)?,
        resolved_at: r.get(8)?,
    })
}

pub fn insert(conn: &Connection, i: &Interaction) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT INTO interactions ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
        ),
        params![
            i.id.to_string(),
            i.chat_id.to_string(),
            i.turn_id.to_string(),
            enum_to_str(&i.kind),
            serde_json::to_string(&i.payload)
                .map_err(|e| crate::StoreError::Other(e.to_string()))?,
            enum_to_str(&i.status),
            i.resolution
                .as_ref()
                .and_then(|r| serde_json::to_string(r).ok()),
            i.created_at,
            i.resolved_at,
        ],
    )?;
    Ok(())
}

/// Records the outcome of a pending interaction.
pub fn resolve(
    conn: &Connection,
    id: InteractionId,
    status: InteractionStatus,
    resolution: &InteractionResolution,
    resolved_at: i64,
) -> Result<bool> {
    let n = conn.execute(
        "UPDATE interactions SET status = ?2, resolution_json = ?3, resolved_at = ?4 WHERE id = ?1",
        params![
            id.to_string(),
            enum_to_str(&status),
            serde_json::to_string(resolution).ok(),
            resolved_at,
        ],
    )?;
    Ok(n > 0)
}

pub fn get(conn: &Connection, id: InteractionId) -> Result<Option<Interaction>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM interactions WHERE id = ?1"),
            params![id.to_string()],
            from_row,
        )
        .optional()?)
}

/// Pending interactions, oldest first, for one chat or for all.
pub fn list_pending(conn: &Connection, chat_id: Option<ChatId>) -> Result<Vec<Interaction>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM interactions WHERE status = 'pending' AND (?1 IS NULL OR chat_id = ?1)
         ORDER BY created_at, rowid"
    ))?;
    let rows = stmt.query_map(params![chat_id.map(|c| c.to_string())], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn list_for_turn(conn: &Connection, turn_id: TurnId) -> Result<Vec<Interaction>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM interactions WHERE turn_id = ?1 ORDER BY created_at, rowid"
    ))?;
    let rows = stmt.query_map(params![turn_id.to_string()], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// `(chat, pending count)` for the sidebar badges.
pub fn pending_counts(conn: &Connection) -> Result<Vec<(ChatId, u32)>> {
    let mut stmt = conn.prepare(
        "SELECT chat_id, count(*) FROM interactions WHERE status = 'pending' GROUP BY chat_id",
    )?;
    let rows = stmt.query_map([], |r| Ok((id_from_str(r, 0)?, r.get::<_, u32>(1)?)))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Crash recovery: nobody can answer a prompt from a process that is gone.
pub fn cancel_pending(conn: &Connection, now: i64) -> Result<usize> {
    let resolution = serde_json::to_string(&InteractionResolution::Cancelled).unwrap_or_default();
    Ok(conn.execute(
        "UPDATE interactions SET status = 'cancelled', resolution_json = ?2, resolved_at = ?1
         WHERE status = 'pending'",
        params![now, resolution],
    )?)
}
