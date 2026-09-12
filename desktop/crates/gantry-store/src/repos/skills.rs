//! `skills`, `skill_versions` and the two pin tables (docs/plan/06 §3, 12 §A3).
//!
//! The index is a projection: the truth of a user skill is the file on disk, and of a bundled
//! one the binary. Everything here exists so matching, the list and the pins can be answered
//! without touching the filesystem, and so a Replace is reversible.

use gantry_core::{ChatId, ProjectId, SkillDto, SkillSource, SkillVersionSource, now_ms};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    db::Result,
    repos::{enum_from_str, enum_to_str},
};

const COLUMNS: &str = "id, source, path, name, description, triggers_json, always_include, \
                       enabled, content_hash, size, version, author, license, references_json, \
                       installed_at, updated_at, last_used_at, use_count";

/// Every skill, bundled first and then by name — the order the list is read in.
pub fn list(conn: &Connection) -> Result<Vec<SkillDto>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS}, \
         (SELECT count(*) FROM chat_skills WHERE skill_id = skills.id) + \
         (SELECT count(*) FROM project_skills WHERE skill_id = skills.id) \
         FROM skills ORDER BY name"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// The enabled skills the matcher scores against (12 §A4).
pub fn enabled(conn: &Connection) -> Result<Vec<SkillDto>> {
    Ok(list(conn)?.into_iter().filter(|s| s.enabled).collect())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<SkillDto>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS}, \
         (SELECT count(*) FROM chat_skills WHERE skill_id = skills.id) + \
         (SELECT count(*) FROM project_skills WHERE skill_id = skills.id) \
         FROM skills WHERE id = ?1"
    ))?;
    Ok(stmt.query_row(params![id], from_row).optional()?)
}

/// Writes the index row, replacing whatever was there. The caller has already written the
/// file (or read it from the binary); this is the projection catching up.
pub fn upsert(conn: &Connection, skill: &SkillDto) -> Result<()> {
    conn.execute(
        "INSERT INTO skills (id, source, path, name, description, triggers_json, \
         always_include, enabled, content_hash, size, version, author, license, \
         references_json, installed_at, updated_at, last_used_at, use_count) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18) \
         ON CONFLICT (id) DO UPDATE SET \
         source = excluded.source, path = excluded.path, name = excluded.name, \
         description = excluded.description, triggers_json = excluded.triggers_json, \
         always_include = excluded.always_include, enabled = excluded.enabled, \
         content_hash = excluded.content_hash, size = excluded.size, \
         version = excluded.version, author = excluded.author, license = excluded.license, \
         references_json = excluded.references_json, updated_at = excluded.updated_at",
        params![
            skill.id,
            enum_to_str(&skill.source),
            skill.path,
            skill.name,
            skill.description,
            serde_json::to_string(&skill.triggers).unwrap_or_else(|_| "[]".to_owned()),
            skill.always_include,
            skill.enabled,
            skill.content_hash,
            skill.size,
            skill.version,
            skill.author,
            skill.license,
            serde_json::to_string(&skill.references).unwrap_or_else(|_| "[]".to_owned()),
            skill.installed_at,
            skill.updated_at,
            skill.last_used_at,
            skill.use_count,
        ],
    )?;
    Ok(())
}

/// The enabled switch is the one field of a bundled skill a user can change.
pub fn set_enabled(conn: &Connection, id: &str, enabled: bool) -> Result<()> {
    conn.execute(
        "UPDATE skills SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, enabled, now_ms()],
    )?;
    Ok(())
}

/// Drops the index row. The file is the caller's to remove; a bundled skill has none.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM skills WHERE id = ?1", params![id])?;
    Ok(())
}

/// Counted when a skill actually reached a prompt (12 §A3), so "last used" is honest.
pub fn mark_used(conn: &Connection, ids: &[String]) -> Result<()> {
    let now = now_ms();
    for id in ids {
        conn.execute(
            "UPDATE skills SET use_count = use_count + 1, last_used_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
    }
    Ok(())
}

/// Snapshots the whole file. Returns the version it wrote.
pub fn add_version(
    conn: &Connection,
    skill_id: &str,
    version: u32,
    content: &str,
    source: SkillVersionSource,
) -> Result<u32> {
    conn.execute(
        "INSERT INTO skill_versions (id, skill_id, version, content, source, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT (skill_id, version) DO UPDATE SET content = excluded.content, \
         source = excluded.source, created_at = excluded.created_at",
        params![
            gantry_core::EventId::new().to_string(),
            skill_id,
            version,
            content,
            enum_to_str(&source),
            now_ms(),
        ],
    )?;
    Ok(version)
}

/// The highest version recorded for a skill, or zero when it has never been saved.
pub fn last_version(conn: &Connection, skill_id: &str) -> Result<u32> {
    Ok(conn
        .query_row(
            "SELECT coalesce(max(version), 0) FROM skill_versions WHERE skill_id = ?1",
            params![skill_id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

/// One stored version's text, for a restore or a diff.
pub fn version_content(conn: &Connection, skill_id: &str, version: u32) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT content FROM skill_versions WHERE skill_id = ?1 AND version = ?2",
            params![skill_id, version],
            |r| r.get(0),
        )
        .optional()?)
}

/// The versions of a skill, newest first: `(version, source, created_at)`.
pub fn versions(conn: &Connection, skill_id: &str) -> Result<Vec<(u32, SkillVersionSource, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT version, source, created_at FROM skill_versions WHERE skill_id = ?1 \
         ORDER BY version DESC",
    )?;
    let rows = stmt.query_map(params![skill_id], |r| {
        Ok((r.get(0)?, enum_from_str(r, 1)?, r.get(2)?))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

// --- Pins -------------------------------------------------------------------------------

pub fn pin_to_chat(conn: &Connection, chat_id: ChatId, skill_id: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO chat_skills (chat_id, skill_id, pinned_at) VALUES (?1, ?2, ?3)",
        params![chat_id.to_string(), skill_id, now_ms()],
    )?;
    Ok(())
}

pub fn unpin_from_chat(conn: &Connection, chat_id: ChatId, skill_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM chat_skills WHERE chat_id = ?1 AND skill_id = ?2",
        params![chat_id.to_string(), skill_id],
    )?;
    Ok(())
}

/// The skills pinned to a chat, oldest pin first.
pub fn pinned_for_chat(conn: &Connection, chat_id: ChatId) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT skill_id FROM chat_skills WHERE chat_id = ?1 ORDER BY pinned_at, skill_id",
    )?;
    let rows = stmt.query_map(params![chat_id.to_string()], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn pinned_for_project(conn: &Connection, project_id: ProjectId) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT skill_id FROM project_skills WHERE project_id = ?1 ORDER BY pinned_at, skill_id",
    )?;
    let rows = stmt.query_map(params![project_id.to_string()], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<SkillDto> {
    let triggers: String = row.get(5)?;
    let references: String = row.get(13)?;
    Ok(SkillDto {
        id: row.get(0)?,
        source: enum_from_str::<SkillSource>(row, 1)?,
        path: row.get(2)?,
        name: row.get(3)?,
        description: row.get(4)?,
        triggers: serde_json::from_str(&triggers).unwrap_or_default(),
        always_include: row.get(6)?,
        enabled: row.get(7)?,
        content_hash: row.get(8)?,
        size: row.get(9)?,
        version: row.get(10)?,
        author: row.get(11)?,
        license: row.get(12)?,
        references: serde_json::from_str(&references).unwrap_or_default(),
        installed_at: row.get(14)?,
        updated_at: row.get(15)?,
        last_used_at: row.get(16)?,
        use_count: row.get(17)?,
        pinned_count: row.get(18)?,
    })
}
