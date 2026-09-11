//! The `tool_calls` projection (docs/plan/06 §3): one row per call the model made, kept in
//! step with the events log by the persister. The result content itself stays in the
//! transcript (the `Tool` message); the row keeps a text preview.

use gantry_core::{
    CallId, ChatId, DecisionSource, RiskTier, ToolCallDto, ToolCallStatus, ToolDisplay, TurnId,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    db::Result,
    repos::{enum_from_str, enum_to_str, id_from_str},
};

const COLUMNS: &str = "id, chat_id, turn_id, message_id, instance_id, connector_name, tool_name, model_tool_name, args_json, tier, status, decision_source, display_json, result_preview, is_error, created_at, started_at, ended_at, duration_ms, judge_json";

fn from_row(r: &Row<'_>) -> rusqlite::Result<ToolCallDto> {
    let args: String = r.get(8)?;
    let display: String = r.get(12)?;
    let decision: Option<String> = r.get(11)?;
    Ok(ToolCallDto {
        id: CallId(r.get(0)?),
        chat_id: id_from_str(r, 1)?,
        turn_id: id_from_str(r, 2)?,
        message_id: id_from_str(r, 3)?,
        connector: r.get(4)?,
        connector_name: r.get(5)?,
        tool: r.get(6)?,
        model_tool_name: r.get(7)?,
        args: serde_json::from_str(&args).map_err(|e| conv(8, e))?,
        tier: enum_from_str::<RiskTier>(r, 9)?,
        status: enum_from_str::<ToolCallStatus>(r, 10)?,
        decision_source: decision
            .map(|s| serde_json::from_value::<DecisionSource>(serde_json::Value::String(s)))
            .transpose()
            .map_err(|e| conv(11, e))?,
        judge: r
            .get::<_, Option<String>>(19)?
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| conv(19, e))?,
        display: serde_json::from_str::<ToolDisplay>(&display).map_err(|e| conv(12, e))?,
        result_preview: r.get(13)?,
        result: None,
        is_error: r.get::<_, i64>(14)? != 0,
        started_at: r.get(16)?,
        ended_at: r.get(17)?,
        duration_ms: r
            .get::<_, Option<i64>>(18)?
            .map(|d| u64::try_from(d).unwrap_or(0)),
    })
}

fn conv(idx: usize, e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
}

pub fn insert(conn: &Connection, c: &ToolCallDto, created_at: i64) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT INTO tool_calls ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)"
        ),
        params![
            c.id.as_str(),
            c.chat_id.to_string(),
            c.turn_id.to_string(),
            c.message_id.to_string(),
            c.connector,
            c.connector_name,
            c.tool,
            c.model_tool_name,
            c.args.to_string(),
            enum_to_str(&c.tier),
            enum_to_str(&c.status),
            c.decision_source.map(|d| enum_to_str(&d)),
            serde_json::to_string(&c.display).unwrap_or_else(|_| "{}".into()),
            c.result_preview,
            i64::from(c.is_error),
            created_at,
            c.started_at,
            c.ended_at,
            c.duration_ms.map(|d| i64::try_from(d).unwrap_or(i64::MAX)),
            judge_json(c),
        ],
    )?;
    Ok(())
}

/// The guard's verdict as the column stores it (04 §11), and `NULL` when no guard was asked.
fn judge_json(c: &ToolCallDto) -> Option<String> {
    c.judge.as_ref().and_then(|j| serde_json::to_string(j).ok())
}

/// Rewrites every mutable column of an existing call.
pub fn update(conn: &Connection, c: &ToolCallDto) -> Result<()> {
    conn.execute(
        "UPDATE tool_calls SET args_json = ?2, tier = ?3, status = ?4, decision_source = ?5, display_json = ?6,
           result_preview = ?7, is_error = ?8, started_at = ?9, ended_at = ?10, duration_ms = ?11,
           judge_json = ?12
         WHERE id = ?1",
        params![
            c.id.as_str(),
            c.args.to_string(),
            enum_to_str(&c.tier),
            enum_to_str(&c.status),
            c.decision_source.map(|d| enum_to_str(&d)),
            serde_json::to_string(&c.display).unwrap_or_else(|_| "{}".into()),
            c.result_preview,
            i64::from(c.is_error),
            c.started_at,
            c.ended_at,
            c.duration_ms.map(|d| i64::try_from(d).unwrap_or(i64::MAX)),
            judge_json(c),
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &CallId) -> Result<Option<ToolCallDto>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM tool_calls WHERE id = ?1"),
            params![id.as_str()],
            from_row,
        )
        .optional()?)
}

pub fn list_for_turn(conn: &Connection, turn_id: TurnId) -> Result<Vec<ToolCallDto>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM tool_calls WHERE turn_id = ?1 ORDER BY created_at, rowid"
    ))?;
    let rows = stmt.query_map(params![turn_id.to_string()], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn list_for_chat(conn: &Connection, chat_id: ChatId) -> Result<Vec<ToolCallDto>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM tool_calls WHERE chat_id = ?1 ORDER BY created_at, rowid"
    ))?;
    let rows = stmt.query_map(params![chat_id.to_string()], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Crash recovery: every call that had not ended when the previous process died is over.
pub fn cancel_open(conn: &Connection, now: i64) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE tool_calls SET status = 'cancelled', ended_at = ?1
         WHERE status IN ('proposed', 'awaiting_decision', 'running')",
        params![now],
    )?)
}
