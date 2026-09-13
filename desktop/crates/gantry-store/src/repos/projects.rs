//! `projects` and `project_files` (docs/plan/06 §3, 09 M11).
//!
//! Two reads matter. The **page** wants everything: instructions, defaults, files, pinned
//! skills. **Chat creation** wants the defaults and the knowledge text, and wants them in one
//! transaction with the chat it is about to write, so that a project edited in another window
//! cannot land half of itself in a new chat's prompt.
//!
//! Deleting a project is the one operation with a rule worth stating: it takes the project's own
//! rows and releases its chats. A chat held a conversation; the project was where it was filed.

use gantry_core::{
    Mode, ProjectDefaults, ProjectDetail, ProjectFileDto, ProjectFileId, ProjectGrant, ProjectId,
    ProjectSummary, now_ms,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    db::Result,
    repos::{enum_from_str, enum_to_str, id_from_str},
};

const COLUMNS: &str = "id, name, description, instructions, workspace_path, default_mode, \
                       default_guard, default_connectors_json, default_grants_json, pinned, \
                       sort_order, created_at, updated_at, archived_at";

/// A project row as it is stored, defaults still in their columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: ProjectId,
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub workspace_path: Option<String>,
    pub defaults: ProjectDefaults,
    pub pinned: bool,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived_at: Option<i64>,
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<ProjectRecord> {
    let mode: Option<String> = r.get(5)?;
    let mode = match mode {
        Some(_) => Some(enum_from_str::<Mode>(r, 5)?),
        None => None,
    };
    Ok(ProjectRecord {
        id: id_from_str(r, 0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        instructions: r.get(3)?,
        workspace_path: r.get(4)?,
        defaults: ProjectDefaults {
            mode,
            guard: r.get(6)?,
            connectors: r
                .get::<_, Option<String>>(7)?
                .and_then(|j| serde_json::from_str(&j).ok()),
            grants: r
                .get::<_, Option<String>>(8)?
                .and_then(|j| serde_json::from_str(&j).ok()),
        },
        pinned: r.get(9)?,
        sort_order: r.get(10)?,
        created_at: r.get(11)?,
        updated_at: r.get(12)?,
        archived_at: r.get(13)?,
    })
}

pub fn insert(conn: &Connection, p: &ProjectRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO projects (id, name, description, instructions, workspace_path, \
         default_mode, default_guard, default_connectors_json, default_grants_json, pinned, \
         sort_order, created_at, updated_at, archived_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            p.id.to_string(),
            p.name,
            p.description,
            p.instructions,
            p.workspace_path,
            p.defaults.mode.as_ref().map(enum_to_str),
            p.defaults.guard,
            json_or_null(p.defaults.connectors.as_ref()),
            json_or_null(p.defaults.grants.as_ref()),
            p.pinned,
            p.sort_order,
            p.created_at,
            p.updated_at,
            p.archived_at,
        ],
    )?;
    Ok(())
}

pub fn update(conn: &Connection, p: &ProjectRecord) -> Result<()> {
    conn.execute(
        "UPDATE projects SET name = ?2, description = ?3, instructions = ?4, \
         workspace_path = ?5, default_mode = ?6, default_guard = ?7, \
         default_connectors_json = ?8, default_grants_json = ?9, pinned = ?10, \
         sort_order = ?11, updated_at = ?12, archived_at = ?13 WHERE id = ?1",
        params![
            p.id.to_string(),
            p.name,
            p.description,
            p.instructions,
            p.workspace_path,
            p.defaults.mode.as_ref().map(enum_to_str),
            p.defaults.guard,
            json_or_null(p.defaults.connectors.as_ref()),
            json_or_null(p.defaults.grants.as_ref()),
            p.pinned,
            p.sort_order,
            p.updated_at,
            p.archived_at,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: ProjectId) -> Result<Option<ProjectRecord>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM projects WHERE id = ?1"),
            params![id.to_string()],
            from_row,
        )
        .optional()?)
}

/// Every project, pinned first, then most recently touched. Archived ones are left out: a
/// project is archived to get it out of the way.
pub fn list(conn: &Connection) -> Result<Vec<ProjectSummary>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS}, \
         (SELECT count(*) FROM chats WHERE project_id = projects.id AND archived_at IS NULL \
          AND incognito = 0), \
         (SELECT count(*) FROM project_files WHERE project_id = projects.id) \
         FROM projects WHERE archived_at IS NULL \
         ORDER BY pinned DESC, sort_order, updated_at DESC"
    ))?;
    let rows = stmt.query_map([], |r| {
        let record = from_row(r)?;
        Ok(summary(&record, r.get(14)?, r.get(15)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[must_use]
pub fn summary(p: &ProjectRecord, chat_count: u32, file_count: u32) -> ProjectSummary {
    ProjectSummary {
        id: p.id,
        name: p.name.clone(),
        description: p.description.clone(),
        pinned: p.pinned,
        archived: p.archived_at.is_some(),
        workspace_path: p.workspace_path.clone(),
        chat_count,
        file_count,
        created_at: p.created_at,
        updated_at: p.updated_at,
    }
}

/// The whole page: the row, its files and its pinned skills.
pub fn detail(conn: &Connection, id: ProjectId) -> Result<Option<ProjectDetail>> {
    let Some(p) = get(conn, id)? else {
        return Ok(None);
    };
    Ok(Some(ProjectDetail {
        id: p.id,
        name: p.name.clone(),
        description: p.description.clone(),
        instructions: p.instructions.clone(),
        workspace_path: p.workspace_path.clone(),
        defaults: p.defaults.clone(),
        pinned: p.pinned,
        archived: p.archived_at.is_some(),
        files: files(conn, id)?,
        skills: crate::repos::skills::pinned_for_project(conn, id)?,
        created_at: p.created_at,
        updated_at: p.updated_at,
    }))
}

/// Deletes the project and releases its chats (see the module note and migration 0014).
pub fn delete(conn: &Connection, id: ProjectId) -> Result<()> {
    conn.execute(
        "UPDATE chats SET project_id = NULL WHERE project_id = ?1",
        params![id.to_string()],
    )?;
    conn.execute(
        "UPDATE artifacts SET project_id = NULL WHERE project_id = ?1",
        params![id.to_string()],
    )?;
    // A project-scoped memory outlives nothing: it was only ever offered to chats in this
    // project, so it is archived rather than left to apply to nobody. Recently deleted still
    // holds it for thirty days (12 §B5).
    conn.execute(
        "UPDATE memories SET archived_at = ?2 WHERE scope_id = ?1 AND archived_at IS NULL",
        params![id.to_string(), now_ms()],
    )?;
    conn.execute(
        "DELETE FROM projects WHERE id = ?1",
        params![id.to_string()],
    )?;
    Ok(())
}

// --- Knowledge files ------------------------------------------------------------------------

pub fn files(conn: &Connection, project_id: ProjectId) -> Result<Vec<ProjectFileDto>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, name, mime, size, blob_hash, length(extracted_text), created_at \
         FROM project_files WHERE project_id = ?1 ORDER BY created_at, name",
    )?;
    let rows = stmt.query_map(params![project_id.to_string()], |r| {
        Ok(ProjectFileDto {
            id: id_from_str(r, 0)?,
            project_id: id_from_str(r, 1)?,
            name: r.get(2)?,
            mime: r.get(3)?,
            size: r.get(4)?,
            blob_hash: r.get(5)?,
            text_chars: r.get::<_, Option<u32>>(6)?.unwrap_or(0),
            created_at: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The text of every knowledge file, in the order they were added, for the prompt.
pub fn knowledge(conn: &Connection, project_id: ProjectId) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT name, extracted_text FROM project_files \
         WHERE project_id = ?1 AND extracted_text IS NOT NULL ORDER BY created_at, name",
    )?;
    let rows = stmt.query_map(params![project_id.to_string()], |r| {
        Ok((r.get(0)?, r.get::<_, String>(1)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// One file's text, for a note sent to a chat that is already open.
pub fn file_text(conn: &Connection, file_id: ProjectFileId) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT extracted_text FROM project_files WHERE id = ?1",
            params![file_id.to_string()],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

pub fn add_file(conn: &Connection, f: &ProjectFileDto, text: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO project_files (id, project_id, name, mime, size, blob_hash, \
         extracted_text, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            f.id.to_string(),
            f.project_id.to_string(),
            f.name,
            f.mime,
            f.size,
            f.blob_hash,
            text,
            f.created_at,
        ],
    )?;
    touch(conn, f.project_id)?;
    Ok(())
}

pub fn remove_file(conn: &Connection, project_id: ProjectId, file_id: ProjectFileId) -> Result<()> {
    conn.execute(
        "DELETE FROM project_files WHERE id = ?1 AND project_id = ?2",
        params![file_id.to_string(), project_id.to_string()],
    )?;
    touch(conn, project_id)?;
    Ok(())
}

/// Marks the project as changed, so the list's order reflects what the user just did.
pub fn touch(conn: &Connection, id: ProjectId) -> Result<()> {
    conn.execute(
        "UPDATE projects SET updated_at = ?2 WHERE id = ?1",
        params![id.to_string(), now_ms()],
    )?;
    Ok(())
}

fn json_or_null<T: serde::Serialize>(value: Option<&T>) -> Option<String> {
    value.and_then(|v| serde_json::to_string(v).ok())
}

/// One project's chats, newest activity first. Incognito chats are in no list (15 A21), and a
/// project is a list.
pub fn chat_ids(conn: &Connection, project_id: ProjectId) -> Result<Vec<gantry_core::ChatId>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM chats WHERE project_id = ?1 AND incognito = 0 \
         ORDER BY last_message_at DESC",
    )?;
    let rows = stmt.query_map(params![project_id.to_string()], |r| id_from_str(r, 0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Moves a chat into a project, or out of every project, keeping `artifacts.project_id` in step
/// (13 §9: an artifact is visible across the project its chat is in).
pub fn set_chat_project(
    conn: &Connection,
    chat_id: gantry_core::ChatId,
    project_id: Option<ProjectId>,
) -> Result<()> {
    let id = project_id.map(|p| p.to_string());
    conn.execute(
        "UPDATE chats SET project_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![chat_id.to_string(), id, now_ms()],
    )?;
    crate::repos::artifacts::set_project_for_chat(conn, chat_id, id.as_deref())?;
    if let Some(project) = project_id {
        touch(conn, project)?;
    }
    Ok(())
}

#[must_use]
pub fn grants_for(defaults: &ProjectDefaults) -> &[ProjectGrant] {
    defaults.grants.as_deref().unwrap_or_default()
}
