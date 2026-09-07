//! The `chats` table (docs/plan/06 §3): one row per conversation with its settings and the
//! frozen system prompt. The transcript lives in `messages`, the turns in `turns`.

use gantry_core::{ChatId, Mode, ModelRef, ProjectId, ProviderId, ReasoningEffort, now_ms};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    db::Result,
    repos::{enum_from_str, enum_to_str, id_from_str},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRecord {
    pub id: ChatId,
    pub project_id: Option<ProjectId>,
    pub title: String,
    /// `auto` (the first words, then the title generator) or `user` (renamed by hand).
    pub title_source: String,
    pub pinned: bool,
    pub mode: Mode,
    pub guard: bool,
    pub model: ModelRef,
    pub effort: ReasoningEffort,
    pub web_search: bool,
    pub instructions: String,
    pub system_snapshot: String,
    pub system_snapshot_version: u32,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_message_at: i64,
    pub archived_at: Option<i64>,
}

const COLUMNS: &str = "id, project_id, title, title_source, pinned, permission_mode, auto_guard, provider_id, model_id, effort, web_search, instructions, system_snapshot, system_snapshot_version, created_at, updated_at, last_message_at, archived_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<ChatRecord> {
    Ok(ChatRecord {
        id: id_from_str(r, 0)?,
        project_id: r
            .get::<_, Option<String>>(1)?
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
        title: r.get(2)?,
        title_source: r.get(3)?,
        pinned: r.get::<_, i64>(4)? != 0,
        mode: enum_from_str(r, 5)?,
        guard: r.get::<_, i64>(6)? != 0,
        model: ModelRef {
            provider: ProviderId::new(r.get::<_, String>(7)?),
            model: r.get(8)?,
        },
        effort: enum_from_str(r, 9)?,
        web_search: r.get::<_, i64>(10)? != 0,
        instructions: r.get(11)?,
        system_snapshot: r.get(12)?,
        system_snapshot_version: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
        last_message_at: r.get(16)?,
        archived_at: r.get(17)?,
    })
}

pub fn insert(conn: &Connection, c: &ChatRecord) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT INTO chats ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)"
        ),
        params![
            c.id.to_string(),
            c.project_id.map(|p| p.to_string()),
            c.title,
            c.title_source,
            c.pinned as i64,
            enum_to_str(&c.mode),
            c.guard as i64,
            c.model.provider.as_str(),
            c.model.model,
            enum_to_str(&c.effort),
            c.web_search as i64,
            c.instructions,
            c.system_snapshot,
            c.system_snapshot_version,
            c.created_at,
            c.updated_at,
            c.last_message_at,
            c.archived_at,
        ],
    )?;
    Ok(())
}

/// Rewrites every mutable column of an existing row and bumps `updated_at`.
pub fn update(conn: &Connection, c: &ChatRecord) -> Result<()> {
    conn.execute(
        "UPDATE chats SET project_id = ?2, title = ?3, title_source = ?4, pinned = ?5, permission_mode = ?6,
           auto_guard = ?7, provider_id = ?8, model_id = ?9, effort = ?10, web_search = ?11, instructions = ?12,
           system_snapshot = ?13, system_snapshot_version = ?14, updated_at = ?15, last_message_at = ?16,
           archived_at = ?17
         WHERE id = ?1",
        params![
            c.id.to_string(),
            c.project_id.map(|p| p.to_string()),
            c.title,
            c.title_source,
            c.pinned as i64,
            enum_to_str(&c.mode),
            c.guard as i64,
            c.model.provider.as_str(),
            c.model.model,
            enum_to_str(&c.effort),
            c.web_search as i64,
            c.instructions,
            c.system_snapshot,
            c.system_snapshot_version,
            now_ms(),
            c.last_message_at,
            c.archived_at,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: ChatId) -> Result<Option<ChatRecord>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM chats WHERE id = ?1"),
            params![id.to_string()],
            from_row,
        )
        .optional()?)
}

/// Every chat, archived ones included, most recent first.
pub fn list(conn: &Connection) -> Result<Vec<ChatRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM chats ORDER BY last_message_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Deletes the chat and, through the foreign keys, its turns, messages, attachments and events.
/// Returns whether a row existed.
pub fn delete(conn: &Connection, id: ChatId) -> Result<bool> {
    let n = conn.execute("DELETE FROM chats WHERE id = ?1", params![id.to_string()])?;
    Ok(n > 0)
}

pub fn set_last_message_at(conn: &Connection, id: ChatId, at: i64) -> Result<()> {
    conn.execute(
        "UPDATE chats SET last_message_at = ?2, updated_at = ?2 WHERE id = ?1",
        params![id.to_string(), at],
    )?;
    Ok(())
}

/// Sets an automatic title unless the user renamed the chat.
pub fn set_auto_title(conn: &Connection, id: ChatId, title: &str) -> Result<bool> {
    let n = conn.execute(
        "UPDATE chats SET title = ?2, updated_at = ?3 WHERE id = ?1 AND title_source = 'auto'",
        params![id.to_string(), title, now_ms()],
    )?;
    Ok(n > 0)
}
