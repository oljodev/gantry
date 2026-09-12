//! `memories` and `memories_fts` (docs/plan/06 §3, 12 §B).
//!
//! Two reads matter and they are different shapes. The **core set** is a scope query — every
//! standing instruction and preference that applies — and it is frozen into a chat's prompt
//! once. The **long tail** is a full-text query per message. Both are capped by the caller;
//! this module only fetches in the right order, so a cap takes the entries worth keeping.

use gantry_core::{
    MemoryDto, MemoryId, MemoryKind, MemoryScopeKind, MemorySource, ProjectId, now_ms,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    db::Result,
    repos::{enum_from_str, enum_to_str, id_from_str, search::fts_query},
};

const COLUMNS: &str = "id, scope_kind, scope_id, kind, text, always_include, source, \
                       origin_chat_id, origin_message_id, tags_json, enabled, use_count, \
                       last_used_at, created_at, updated_at, archived_at";

/// How the Memory page reads the store: live entries newest first, or the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryFilter {
    pub scope_kind: Option<MemoryScopeKind>,
    pub kind: Option<MemoryKind>,
    pub source: Option<MemorySource>,
    pub enabled: Option<bool>,
    /// `true` lists Recently deleted instead of the live set.
    pub archived: bool,
}

pub fn insert(conn: &Connection, m: &MemoryDto) -> Result<()> {
    conn.execute(
        "INSERT INTO memories (id, scope_kind, scope_id, kind, text, always_include, source, \
         origin_chat_id, origin_message_id, tags_json, enabled, use_count, last_used_at, \
         created_at, updated_at, archived_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            m.id.to_string(),
            enum_to_str(&m.scope_kind),
            m.scope_id.map(|p| p.to_string()),
            enum_to_str(&m.kind),
            m.text,
            m.always_include,
            enum_to_str(&m.source),
            m.origin_chat_id.map(|c| c.to_string()),
            m.origin_message_id.map(|c| c.to_string()),
            serde_json::to_string(&m.tags).unwrap_or_else(|_| "[]".to_owned()),
            m.enabled,
            m.use_count,
            m.last_used_at,
            m.created_at,
            m.updated_at,
            m.archived_at,
        ],
    )?;
    Ok(())
}

pub fn update(conn: &Connection, m: &MemoryDto) -> Result<()> {
    conn.execute(
        "UPDATE memories SET scope_kind = ?2, scope_id = ?3, kind = ?4, text = ?5, \
         always_include = ?6, tags_json = ?7, enabled = ?8, updated_at = ?9, archived_at = ?10 \
         WHERE id = ?1",
        params![
            m.id.to_string(),
            enum_to_str(&m.scope_kind),
            m.scope_id.map(|p| p.to_string()),
            enum_to_str(&m.kind),
            m.text,
            m.always_include,
            serde_json::to_string(&m.tags).unwrap_or_else(|_| "[]".to_owned()),
            m.enabled,
            m.updated_at,
            m.archived_at,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: MemoryId) -> Result<Option<MemoryDto>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM memories WHERE id = ?1"))?;
    Ok(stmt
        .query_row(params![id.to_string()], from_row)
        .optional()?)
}

/// The Memory page's list. `query` runs through FTS when it is not empty.
pub fn list(conn: &Connection, filter: MemoryFilter, query: &str) -> Result<Vec<MemoryDto>> {
    let mut sql = format!("SELECT {COLUMNS} FROM memories WHERE ");
    sql.push_str(if filter.archived {
        "archived_at IS NOT NULL"
    } else {
        "archived_at IS NULL"
    });
    if let Some(s) = filter.scope_kind {
        sql.push_str(&format!(" AND scope_kind = '{}'", enum_to_str(&s)));
    }
    if let Some(k) = filter.kind {
        sql.push_str(&format!(" AND kind = '{}'", enum_to_str(&k)));
    }
    if let Some(s) = filter.source {
        sql.push_str(&format!(" AND source = '{}'", enum_to_str(&s)));
    }
    if let Some(e) = filter.enabled {
        sql.push_str(if e {
            " AND enabled = 1"
        } else {
            " AND enabled = 0"
        });
    }
    if let Some(fts) = fts_query(query) {
        sql.push_str(" AND id IN (SELECT memory_id FROM memories_fts WHERE memories_fts MATCH ?1)");
        sql.push_str(" ORDER BY updated_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![fts], from_row)?;
        return Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?);
    }
    sql.push_str(" ORDER BY updated_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// The core set of a chat in this scope (12 §B4): what the user marked Always, plus every
/// standing instruction and preference. Most recently used first, so a cap keeps what is
/// actually being used.
pub fn core_set(conn: &Connection, project: Option<ProjectId>) -> Result<Vec<MemoryDto>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM memories \
         WHERE enabled = 1 AND archived_at IS NULL \
           AND (scope_kind = 'global' OR scope_id = ?1) \
           AND (always_include = 1 OR kind IN ('instruction', 'preference')) \
         ORDER BY coalesce(last_used_at, created_at) DESC"
    ))?;
    let rows = stmt.query_map(params![project.map(|p| p.to_string())], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// The long tail for one message (12 §B4): facts and notes in scope, ranked by BM25.
///
/// `terms` are the message's significant words, already stripped of stopwords by the caller —
/// this is an OR, not an AND, because a message is not a search box: one word in common with
/// an entry is the whole signal, and requiring every word would find nothing.
///
/// The core-set kinds are excluded here rather than allowed to appear twice — they are already
/// in the frozen prompt, and a second copy in the turn block would be the same sentence said
/// twice in one request.
pub fn long_tail(
    conn: &Connection,
    project: Option<ProjectId>,
    terms: &[String],
    limit: usize,
) -> Result<Vec<MemoryDto>> {
    let quoted: Vec<String> = terms
        .iter()
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect();
    if quoted.is_empty() {
        return Ok(Vec::new());
    }
    let fts = quoted.join(" OR ");
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM memories_fts f JOIN memories m ON m.id = f.memory_id \
         WHERE memories_fts MATCH ?1 AND m.enabled = 1 AND m.archived_at IS NULL \
           AND m.always_include = 0 AND m.kind IN ('fact', 'note') \
           AND (m.scope_kind = 'global' OR m.scope_id = ?2) \
         ORDER BY bm25(memories_fts) LIMIT ?3",
        COLUMNS
            .split(", ")
            .map(|c| format!("m.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    ))?;
    let rows = stmt.query_map(
        params![
            fts,
            project.map(|p| p.to_string()),
            i64::try_from(limit).unwrap_or(i64::MAX)
        ],
        from_row,
    )?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Deletes into Recently deleted (12 §B5). The row stays so it can come back.
pub fn archive(conn: &Connection, id: MemoryId) -> Result<()> {
    let now = now_ms();
    conn.execute(
        "UPDATE memories SET archived_at = ?2, updated_at = ?2 WHERE id = ?1",
        params![id.to_string(), now],
    )?;
    Ok(())
}

pub fn restore(conn: &Connection, id: MemoryId) -> Result<()> {
    conn.execute(
        "UPDATE memories SET archived_at = NULL, updated_at = ?2 WHERE id = ?1",
        params![id.to_string(), now_ms()],
    )?;
    Ok(())
}

/// Removes for good: the user emptied Recently deleted, or the thirty days ran out.
pub fn purge(conn: &Connection, before: i64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM memories WHERE archived_at IS NOT NULL AND archived_at < ?1",
        params![before],
    )?)
}

pub fn delete_now(conn: &Connection, id: MemoryId) -> Result<()> {
    conn.execute(
        "DELETE FROM memories WHERE id = ?1",
        params![id.to_string()],
    )?;
    Ok(())
}

/// Counted when an entry reached a prompt, which is what "used in N chats" means (12 §B4).
pub fn mark_used(conn: &Connection, ids: &[MemoryId]) -> Result<()> {
    let now = now_ms();
    for id in ids {
        conn.execute(
            "UPDATE memories SET use_count = use_count + 1, last_used_at = ?2 WHERE id = ?1",
            params![id.to_string(), now],
        )?;
    }
    Ok(())
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<MemoryDto> {
    let scope_id: Option<String> = row.get(2)?;
    let origin_chat: Option<String> = row.get(7)?;
    let origin_message: Option<String> = row.get(8)?;
    let tags: String = row.get(9)?;
    Ok(MemoryDto {
        id: id_from_str(row, 0)?,
        scope_kind: enum_from_str(row, 1)?,
        scope_id: scope_id.and_then(|s| s.parse().ok()),
        kind: enum_from_str(row, 3)?,
        text: row.get(4)?,
        always_include: row.get(5)?,
        source: enum_from_str(row, 6)?,
        origin_chat_id: origin_chat.and_then(|s| s.parse().ok()),
        origin_message_id: origin_message.and_then(|s| s.parse().ok()),
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        enabled: row.get(10)?,
        use_count: row.get(11)?,
        last_used_at: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
        archived_at: row.get(15)?,
    })
}
