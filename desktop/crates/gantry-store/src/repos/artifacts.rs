//! The `artifacts`, `artifact_versions` and `artifacts_fts` tables (docs/plan/13 §7). Content
//! bytes live in the blob store; these rows hold the hash and the bookkeeping.

use gantry_core::{
    ArtifactDto, ArtifactId, ArtifactVersionDto, ChatId, MessageId, VersionSource, now_ms,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    db::Result,
    repos::{enum_from_str, enum_to_str, id_from_str},
};

const ARTIFACT_COLUMNS: &str = "id, chat_id, project_id, type, title, language, summary, current_version, created_by_message_id, created_at, updated_at";

fn artifact_from_row(r: &Row<'_>) -> rusqlite::Result<ArtifactDto> {
    Ok(ArtifactDto {
        id: id_from_str(r, 0)?,
        chat_id: id_from_str(r, 1)?,
        project_id: r.get::<_, Option<String>>(2)?.and_then(|s| s.parse().ok()),
        artifact_type: r.get(3)?,
        title: r.get(4)?,
        language: r.get(5)?,
        summary: r.get(6)?,
        current_version: r.get(7)?,
        created_by_message_id: r.get::<_, Option<String>>(8)?.and_then(|s| s.parse().ok()),
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

const VERSION_COLUMNS: &str =
    "artifact_id, version, source, tool_call_id, message_id, note, size, created_at";

fn version_from_row(r: &Row<'_>) -> rusqlite::Result<ArtifactVersionDto> {
    Ok(ArtifactVersionDto {
        artifact_id: id_from_str(r, 0)?,
        version: r.get(1)?,
        source: enum_from_str(r, 2)?,
        tool_call_id: r.get(3)?,
        message_id: r.get::<_, Option<String>>(4)?.and_then(|s| s.parse().ok()),
        note: r.get(5)?,
        size: r.get::<_, i64>(6)?.max(0) as u64,
        created_at: r.get(7)?,
    })
}

/// What a new version needs besides the artifact it belongs to.
#[derive(Debug, Clone)]
pub struct NewVersion {
    pub content_blob_hash: String,
    pub size: u64,
    pub source: VersionSource,
    pub tool_call_id: Option<String>,
    pub message_id: Option<MessageId>,
    pub note: Option<String>,
    /// The content's text, for the search index only.
    pub text: String,
}

/// Inserts the artifact with its first version and returns the row.
#[allow(clippy::too_many_arguments)]
pub fn create(
    conn: &Connection,
    chat_id: ChatId,
    project_id: Option<String>,
    artifact_type: &str,
    title: &str,
    language: Option<&str>,
    summary: Option<&str>,
    created_by_message_id: Option<MessageId>,
    first: &NewVersion,
) -> Result<ArtifactDto> {
    let id = ArtifactId::new();
    let now = now_ms();
    conn.execute(
        "INSERT INTO artifacts (id, chat_id, project_id, type, title, language, summary, current_version, created_by_message_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?9)",
        params![
            id.to_string(),
            chat_id.to_string(),
            project_id,
            artifact_type,
            title,
            language,
            summary,
            created_by_message_id.map(|m| m.to_string()),
            now
        ],
    )?;
    insert_version(conn, id, 1, first, now)?;
    replace_fts(conn, id, chat_id, title, summary, &first.text)?;
    get(conn, id)?.ok_or_else(|| crate::StoreError::Other("artifact vanished".into()))
}

/// Appends a version, optionally renaming the artifact, and returns the new version number.
pub fn add_version(
    conn: &Connection,
    id: ArtifactId,
    title: Option<&str>,
    summary: Option<&str>,
    next: &NewVersion,
) -> Result<u32> {
    let current = get(conn, id)?
        .ok_or_else(|| crate::StoreError::Other(format!("artifact {id} not found")))?;
    let version = current.current_version + 1;
    let now = now_ms();
    insert_version(conn, id, version, next, now)?;
    let title = title.unwrap_or(&current.title);
    let summary = summary.or(current.summary.as_deref());
    conn.execute(
        "UPDATE artifacts SET current_version = ?2, title = ?3, summary = ?4, updated_at = ?5 WHERE id = ?1",
        params![id.to_string(), version, title, summary, now],
    )?;
    replace_fts(conn, id, current.chat_id, title, summary, &next.text)?;
    Ok(version)
}

fn insert_version(
    conn: &Connection,
    id: ArtifactId,
    version: u32,
    v: &NewVersion,
    now: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO artifact_versions (id, artifact_id, version, content_blob_hash, source, tool_call_id, message_id, note, size, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            gantry_core::EventId::new().to_string(),
            id.to_string(),
            version,
            v.content_blob_hash,
            enum_to_str(&v.source),
            v.tool_call_id,
            v.message_id.map(|m| m.to_string()),
            v.note,
            i64::try_from(v.size).unwrap_or(i64::MAX),
            now
        ],
    )?;
    Ok(())
}

fn replace_fts(
    conn: &Connection,
    id: ArtifactId,
    chat_id: ChatId,
    title: &str,
    summary: Option<&str>,
    text: &str,
) -> Result<()> {
    conn.execute(
        "DELETE FROM artifacts_fts WHERE artifact_id = ?1",
        params![id.to_string()],
    )?;
    conn.execute(
        "INSERT INTO artifacts_fts (artifact_id, chat_id, title, summary, text) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id.to_string(),
            chat_id.to_string(),
            title,
            summary.unwrap_or(""),
            text
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: ArtifactId) -> Result<Option<ArtifactDto>> {
    Ok(conn
        .query_row(
            &format!("SELECT {ARTIFACT_COLUMNS} FROM artifacts WHERE id = ?1"),
            params![id.to_string()],
            artifact_from_row,
        )
        .optional()?)
}

pub fn list_for_chat(conn: &Connection, chat_id: ChatId) -> Result<Vec<ArtifactDto>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {ARTIFACT_COLUMNS} FROM artifacts WHERE chat_id = ?1 AND archived_at IS NULL ORDER BY created_at"
    ))?;
    let rows = stmt.query_map(params![chat_id.to_string()], artifact_from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn list_for_project(conn: &Connection, project_id: &str) -> Result<Vec<ArtifactDto>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {ARTIFACT_COLUMNS} FROM artifacts WHERE project_id = ?1 AND archived_at IS NULL ORDER BY updated_at DESC"
    ))?;
    let rows = stmt.query_map(params![project_id], artifact_from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn versions(conn: &Connection, id: ArtifactId) -> Result<Vec<ArtifactVersionDto>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {VERSION_COLUMNS} FROM artifact_versions WHERE artifact_id = ?1 ORDER BY version"
    ))?;
    let rows = stmt.query_map(params![id.to_string()], version_from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// The content hash of one version.
pub fn content_hash(conn: &Connection, id: ArtifactId, version: u32) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT content_blob_hash FROM artifact_versions WHERE artifact_id = ?1 AND version = ?2",
            params![id.to_string(), version],
            |r| r.get(0),
        )
        .optional()?)
}

/// Every content hash an artifact's versions reference, for the sweep when a chat goes.
pub fn hashes_for_chat(conn: &Connection, chat_id: ChatId) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT v.content_blob_hash FROM artifact_versions v JOIN artifacts a ON a.id = v.artifact_id WHERE a.chat_id = ?1",
    )?;
    let rows = stmt.query_map(params![chat_id.to_string()], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Keeps `artifacts.project_id` in step when a chat moves (13 §9).
pub fn set_project_for_chat(
    conn: &Connection,
    chat_id: ChatId,
    project_id: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE artifacts SET project_id = ?2 WHERE chat_id = ?1",
        params![chat_id.to_string(), project_id],
    )?;
    Ok(())
}
