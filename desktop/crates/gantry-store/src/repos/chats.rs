//! The `chats` table (docs/plan/06 §3): one row per conversation with its settings and the
//! frozen system prompt. The transcript lives in `messages`, the turns in `turns`.

use gantry_core::{
    ChatId, Mode, ModelRef, ProjectId, ProviderId, ReasoningEffort, Surface, TurnId, now_ms,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    db::Result,
    repos::{enum_from_str, enum_to_str, id_from_str},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRecord {
    pub id: ChatId,
    /// Which surface the session belongs to (16 §13). Chosen at creation, never changed.
    pub surface: Surface,
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
    /// Never listed, never searched, deleted with its window (15 A21).
    pub incognito: bool,
    /// The turn that started this chat as a sub agent (18 §8). `None` for a conversation a
    /// person is having; `Some` for one a model is having on their behalf.
    pub parent_turn_id: Option<TurnId>,
    /// Which agent type it was started from, for the tree and for the transcript's header.
    pub agent_type: Option<String>,
}

const COLUMNS: &str = "id, project_id, title, title_source, pinned, permission_mode, auto_guard, provider_id, model_id, effort, web_search, instructions, system_snapshot, system_snapshot_version, created_at, updated_at, last_message_at, archived_at, surface, incognito, parent_turn_id, agent_type";

/// Every list a person can reach leaves incognito sessions out, and sub agents with them: one is
/// a conversation that was promised not to be kept, the other a conversation the user is not
/// having (18 A1). The only things that see either are `get` — the open window, or the tree,
/// asking for one chat by id — and the sweeps that delete them.
const VISIBLE: &str = "incognito = 0 AND parent_turn_id IS NULL";

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
        surface: enum_from_str(r, 18)?,
        incognito: r.get::<_, i64>(19)? != 0,
        parent_turn_id: r
            .get::<_, Option<String>>(20)?
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    20,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
        agent_type: r.get(21)?,
    })
}

pub fn insert(conn: &Connection, c: &ChatRecord) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT INTO chats ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)"
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
            enum_to_str(&c.surface),
            c.incognito as i64,
            c.parent_turn_id.map(|t| t.to_string()),
            c.agent_type.clone(),
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

/// Every session of one surface, archived ones included, most recent first. The two lists never
/// mix: a code session does not appear among the chats and the reverse (16 §6).
pub fn list(conn: &Connection, surface: Surface) -> Result<Vec<ChatRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM chats WHERE surface = ?1 AND {VISIBLE} ORDER BY last_message_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map(params![surface.as_str()], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Every session of either surface, for the places that span both: search, the artifact
/// library, and the sweep that marks turns interrupted at startup.
pub fn list_all(conn: &Connection) -> Result<Vec<ChatRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM chats WHERE {VISIBLE} ORDER BY last_message_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

// ---- Roots ---------------------------------------------------------------------------------

/// The folders a session may reach, oldest first: the first one is its primary folder (16 §7).
/// Which memories the chat's frozen prompt was built from (docs/plan/10 §6, 12 §B4).
///
/// A narrow pair rather than two more fields on `ChatRecord`: nothing about a chat's behaviour
/// reads this. It is provenance — what the snapshot was made of — for the Memory page's line
/// about an entry an open chat still carries, and for developer mode.
pub fn set_snapshot_memories(
    conn: &Connection,
    chat_id: gantry_core::ChatId,
    ids: &[gantry_core::MemoryId],
) -> Result<()> {
    let json = serde_json::to_string(&ids.iter().map(ToString::to_string).collect::<Vec<_>>())
        .unwrap_or_else(|_| "[]".to_owned());
    conn.execute(
        "UPDATE chats SET snapshot_memory_ids_json = ?2 WHERE id = ?1",
        params![chat_id.to_string(), json],
    )?;
    Ok(())
}

pub fn snapshot_memories(
    conn: &Connection,
    chat_id: gantry_core::ChatId,
) -> Result<Vec<gantry_core::MemoryId>> {
    let json: String = conn.query_row(
        "SELECT snapshot_memory_ids_json FROM chats WHERE id = ?1",
        params![chat_id.to_string()],
        |r| r.get(0),
    )?;
    Ok(serde_json::from_str::<Vec<String>>(&json)
        .unwrap_or_default()
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect())
}

pub fn roots(conn: &Connection, chat_id: ChatId) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT path FROM chat_roots WHERE chat_id = ?1 ORDER BY added_at, path")?;
    let rows = stmt.query_map(params![chat_id.to_string()], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Adds a folder. Adding the same one twice is not an error.
pub fn add_root(conn: &Connection, chat_id: ChatId, path: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO chat_roots (chat_id, path, added_at) VALUES (?1, ?2, ?3) \
         ON CONFLICT (chat_id, path) DO NOTHING",
        params![chat_id.to_string(), path, now_ms()],
    )?;
    Ok(())
}

pub fn remove_root(conn: &Connection, chat_id: ChatId, path: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM chat_roots WHERE chat_id = ?1 AND path = ?2",
        params![chat_id.to_string(), path],
    )?;
    Ok(())
}

/// Deletes the chat and, through the foreign keys, its turns, messages, attachments and events.
/// Returns whether a row existed.
pub fn delete(conn: &Connection, id: ChatId) -> Result<bool> {
    let n = conn.execute("DELETE FROM chats WHERE id = ?1", params![id.to_string()])?;
    Ok(n > 0)
}

/// Deletes every incognito session, run at startup and when an incognito window closes (15
/// A21). An incognito chat is meant to live exactly as long as its window; this is what makes
/// that true across a crash, a kill, or a machine that lost power mid-sentence.
pub fn delete_incognito(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM chats WHERE incognito = 1", [])?)
}

/// Deletes sub-agent transcripts older than `cutoff` (18 §9, `settings.subagents.keep_days`).
///
/// Run at startup, like the incognito sweep above. It only ever reaches a chat a model had, and
/// only one whose last message is older than the user's own number: a conversation a person had
/// has no `parent_turn_id` and is never in this query.
pub fn delete_old_sub_agents(conn: &Connection, cutoff: i64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM chats WHERE parent_turn_id IS NOT NULL AND last_message_at < ?1",
        params![cutoff],
    )?)
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
