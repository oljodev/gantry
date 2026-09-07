//! The `turns` table (docs/plan/06 §3): one row per user message and the work it started.

use gantry_core::{ChatId, Feedback, ModelRef, ProviderId, StopReason, TurnId, TurnStatus, Usage};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    db::Result,
    repos::{enum_from_str, enum_to_str, id_from_str},
};

#[derive(Debug, Clone, PartialEq)]
pub struct TurnRecord {
    pub id: TurnId,
    pub chat_id: ChatId,
    /// Position inside the chat, from 1.
    pub seq: u32,
    pub status: TurnStatus,
    pub model: ModelRef,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub usage: Option<Usage>,
    pub stop_reason: Option<StopReason>,
    pub error: Option<String>,
    pub feedback: Option<Feedback>,
    pub tool_call_count: u32,
}

const COLUMNS: &str = "id, chat_id, seq, status, provider_id, model_id, started_at, ended_at, usage_json, stop_reason_json, error_json, feedback, tool_call_count";

fn json_col<T: serde::de::DeserializeOwned>(
    r: &Row<'_>,
    idx: usize,
) -> rusqlite::Result<Option<T>> {
    let raw: Option<String> = r.get(idx)?;
    raw.map(|s| {
        serde_json::from_str(&s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
        })
    })
    .transpose()
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<TurnRecord> {
    let error: Option<serde_json::Value> = json_col(r, 10)?;
    Ok(TurnRecord {
        id: id_from_str(r, 0)?,
        chat_id: id_from_str(r, 1)?,
        seq: r.get(2)?,
        status: enum_from_str(r, 3)?,
        model: ModelRef {
            provider: ProviderId::new(r.get::<_, String>(4)?),
            model: r.get(5)?,
        },
        started_at: r.get(6)?,
        ended_at: r.get(7)?,
        usage: json_col(r, 8)?,
        stop_reason: json_col(r, 9)?,
        error: error.and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_owned)),
        feedback: r
            .get::<_, Option<String>>(11)?
            .map(|s| serde_json::from_value(serde_json::Value::String(s)))
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    11,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
        tool_call_count: r.get(12)?,
    })
}

fn to_json<T: serde::Serialize>(v: &Option<T>) -> Option<String> {
    v.as_ref().and_then(|x| serde_json::to_string(x).ok())
}

pub fn insert(conn: &Connection, t: &TurnRecord) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT INTO turns ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
        ),
        params![
            t.id.to_string(),
            t.chat_id.to_string(),
            t.seq,
            enum_to_str(&t.status),
            t.model.provider.as_str(),
            t.model.model,
            t.started_at,
            t.ended_at,
            to_json(&t.usage),
            to_json(&t.stop_reason),
            t.error
                .as_ref()
                .map(|m| serde_json::json!({ "message": m }).to_string()),
            t.feedback.map(|f| enum_to_str(&f)),
            t.tool_call_count,
        ],
    )?;
    Ok(())
}

/// Rewrites the outcome columns of an existing turn.
pub fn update(conn: &Connection, t: &TurnRecord) -> Result<()> {
    conn.execute(
        "UPDATE turns SET status = ?2, ended_at = ?3, usage_json = ?4, stop_reason_json = ?5, error_json = ?6,
           feedback = ?7, tool_call_count = ?8
         WHERE id = ?1",
        params![
            t.id.to_string(),
            enum_to_str(&t.status),
            t.ended_at,
            to_json(&t.usage),
            to_json(&t.stop_reason),
            t.error
                .as_ref()
                .map(|m| serde_json::json!({ "message": m }).to_string()),
            t.feedback.map(|f| enum_to_str(&f)),
            t.tool_call_count,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: TurnId) -> Result<Option<TurnRecord>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM turns WHERE id = ?1"),
            params![id.to_string()],
            from_row,
        )
        .optional()?)
}

pub fn list_for_chat(conn: &Connection, chat_id: ChatId) -> Result<Vec<TurnRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM turns WHERE chat_id = ?1 ORDER BY seq"
    ))?;
    let rows = stmt.query_map(params![chat_id.to_string()], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// The running turn of a chat, if any.
pub fn running_for_chat(conn: &Connection, chat_id: ChatId) -> Result<Option<TurnRecord>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM turns WHERE chat_id = ?1 AND status = 'running' LIMIT 1"
            ),
            params![chat_id.to_string()],
            from_row,
        )
        .optional()?)
}

/// Chat ids that have a running turn, for the sidebar.
pub fn chats_with_running_turns(conn: &Connection) -> Result<Vec<(ChatId, TurnId)>> {
    let mut stmt = conn.prepare("SELECT chat_id, id FROM turns WHERE status = 'running'")?;
    let rows = stmt.query_map([], |r| Ok((id_from_str(r, 0)?, id_from_str(r, 1)?)))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn next_seq(conn: &Connection, chat_id: ChatId) -> Result<u32> {
    let max: Option<u32> = conn.query_row(
        "SELECT max(seq) FROM turns WHERE chat_id = ?1",
        params![chat_id.to_string()],
        |r| r.get(0),
    )?;
    Ok(max.unwrap_or(0) + 1)
}

pub fn last_for_chat(conn: &Connection, chat_id: ChatId) -> Result<Option<TurnRecord>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM turns WHERE chat_id = ?1 ORDER BY seq DESC LIMIT 1"),
            params![chat_id.to_string()],
            from_row,
        )
        .optional()?)
}

/// Deletes a turn with its messages and events.
pub fn delete(conn: &Connection, id: TurnId) -> Result<()> {
    conn.execute("DELETE FROM turns WHERE id = ?1", params![id.to_string()])?;
    Ok(())
}

/// Crash recovery at startup (docs/plan/09 M2): a turn still `running` when the app starts was
/// cut off by a crash or a kill. Returns how many were marked.
pub fn interrupt_running(conn: &Connection, now: i64) -> Result<usize> {
    let n = conn.execute(
        "UPDATE turns SET status = 'interrupted', ended_at = ?1,
           error_json = json_object('message', 'Gantry was closed while this reply was streaming')
         WHERE status = 'running'",
        params![now],
    )?;
    Ok(n)
}
