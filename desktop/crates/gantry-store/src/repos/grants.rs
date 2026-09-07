//! `chat_grants` (docs/plan/04 §8, 06 §3): the standing permissions of one chat, written when
//! the user picks a scope on a permission card and read on every decision after that.

use gantry_core::{ArgScope, ChatGrant, ChatId, GrantId, GrantSource, RiskTier, now_ms};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    db::Result,
    repos::{enum_from_str, enum_to_str, id_from_str},
};

const COLUMNS: &str = "id, chat_id, instance_id, instance_name, tool_name, tier_ceiling, \
                       arg_scope_json, source, created_at, revoked_at";

pub fn insert(conn: &Connection, grant: &ChatGrant) -> Result<()> {
    conn.execute(
        "INSERT INTO chat_grants (id, chat_id, instance_id, instance_name, tool_name, \
         tier_ceiling, arg_scope_json, source, created_at, revoked_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            grant.id.to_string(),
            grant.chat_id.to_string(),
            grant.instance_id,
            grant.instance_name,
            grant.tool_name,
            grant.tier_ceiling.map(|t| enum_to_str(&t)),
            grant
                .arg_scope
                .as_ref()
                .and_then(|s| serde_json::to_string(s).ok()),
            enum_to_str(&grant.source),
            grant.created_at,
            grant.revoked_at,
        ],
    )?;
    Ok(())
}

/// Every grant of the chat that still stands, oldest first.
pub fn active(conn: &Connection, chat_id: ChatId) -> Result<Vec<ChatGrant>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM chat_grants WHERE chat_id = ?1 AND revoked_at IS NULL \
         ORDER BY created_at"
    ))?;
    let rows = stmt.query_map(params![chat_id.to_string()], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Marks one grant revoked. Returns the chat it belonged to, so the caller can refresh.
pub fn revoke(conn: &Connection, id: GrantId) -> Result<Option<ChatId>> {
    let chat: Option<String> = conn
        .query_row(
            "SELECT chat_id FROM chat_grants WHERE id = ?1",
            params![id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    conn.execute(
        "UPDATE chat_grants SET revoked_at = ?2 WHERE id = ?1 AND revoked_at IS NULL",
        params![id.to_string(), now_ms()],
    )?;
    Ok(chat.and_then(|c| c.parse().ok()))
}

/// Revokes every standing grant of a chat at once (the Permissions panel's "Revoke all").
pub fn revoke_all(conn: &Connection, chat_id: ChatId) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE chat_grants SET revoked_at = ?2 WHERE chat_id = ?1 AND revoked_at IS NULL",
        params![chat_id.to_string(), now_ms()],
    )?)
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<ChatGrant> {
    let tier_ceiling: Option<String> = row.get(5)?;
    let arg_scope: Option<String> = row.get(6)?;
    Ok(ChatGrant {
        id: id_from_str(row, 0)?,
        chat_id: id_from_str(row, 1)?,
        instance_id: row.get(2)?,
        instance_name: row.get(3)?,
        tool_name: row.get(4)?,
        tier_ceiling: tier_ceiling
            .and_then(|t| serde_json::from_value::<RiskTier>(serde_json::Value::String(t)).ok()),
        arg_scope: arg_scope.and_then(|json| serde_json::from_str::<ArgScope>(&json).ok()),
        source: enum_from_str::<GrantSource>(row, 7)?,
        created_at: row.get(8)?,
        revoked_at: row.get(9)?,
    })
}
