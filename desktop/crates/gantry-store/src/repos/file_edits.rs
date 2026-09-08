//! The `file_edits` table: the edit journal (docs/plan/06 §3).
//!
//! One row per change, written by the connector that made it. The before and after contents
//! live in the blob store, which is what lets **Revert** and `undo` put a file back exactly
//! rather than approximately. A revert writes a new row and marks the old one rather than
//! deleting it: the history of what happened stays true.

use gantry_core::{ChatId, EditOp, now_ms};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::Result;

const COLUMNS: &str = "id, tool_call_id, chat_id, path, op, before_blob_hash, after_blob_hash, \
                       hunks_json, stats_json, applied_at, reverted_at, reverted_by_edit_id, \
                       from_path";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEditRecord {
    pub id: String,
    pub tool_call_id: String,
    pub chat_id: ChatId,
    pub path: String,
    pub op: EditOp,
    pub before_blob_hash: Option<String>,
    pub after_blob_hash: Option<String>,
    pub hunks_json: String,
    pub stats_json: String,
    pub applied_at: i64,
    pub reverted_at: Option<i64>,
    pub reverted_by_edit_id: Option<String>,
    /// Where a moved or copied file came from; absent for an edit in place.
    pub from_path: Option<String>,
}

pub fn insert(conn: &Connection, edit: &FileEditRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO file_edits (id, tool_call_id, chat_id, path, op, before_blob_hash, \
         after_blob_hash, hunks_json, stats_json, applied_at, reverted_at, reverted_by_edit_id, \
         from_path) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            edit.id,
            edit.tool_call_id,
            edit.chat_id.to_string(),
            edit.path,
            edit.op.as_str(),
            edit.before_blob_hash,
            edit.after_blob_hash,
            edit.hunks_json,
            edit.stats_json,
            edit.applied_at,
            edit.reverted_at,
            edit.reverted_by_edit_id,
            edit.from_path,
        ],
    )?;
    Ok(())
}

/// Every edit to one file in one chat, oldest first.
pub fn for_path(conn: &Connection, chat_id: ChatId, path: &str) -> Result<Vec<FileEditRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM file_edits WHERE chat_id = ?1 AND path = ?2 ORDER BY applied_at, id"
    ))?;
    let rows = stmt.query_map(params![chat_id.to_string(), path], read)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Every edit in one chat, oldest first: the Changes pane, and whole-session Revert.
pub fn for_chat(conn: &Connection, chat_id: ChatId) -> Result<Vec<FileEditRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM file_edits WHERE chat_id = ?1 ORDER BY applied_at, id"
    ))?;
    let rows = stmt.query_map(params![chat_id.to_string()], read)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<FileEditRecord>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM file_edits WHERE id = ?1"),
            params![id],
            read,
        )
        .optional()?)
}

/// Marks edits as undone by the edit that undid them.
pub fn mark_reverted(conn: &Connection, ids: &[String], by: &str) -> Result<()> {
    let now = now_ms();
    for id in ids {
        conn.execute(
            "UPDATE file_edits SET reverted_at = ?2, reverted_by_edit_id = ?3 \
             WHERE id = ?1 AND reverted_at IS NULL",
            params![id, now, by],
        )?;
    }
    Ok(())
}

fn read(r: &Row<'_>) -> rusqlite::Result<FileEditRecord> {
    let op: String = r.get(4)?;
    Ok(FileEditRecord {
        id: r.get(0)?,
        tool_call_id: r.get(1)?,
        chat_id: super::id_from_str(r, 2)?,
        path: r.get(3)?,
        op: EditOp::parse(&op).unwrap_or(EditOp::Modify),
        before_blob_hash: r.get(5)?,
        after_blob_hash: r.get(6)?,
        hunks_json: r.get(7)?,
        stats_json: r.get(8)?,
        applied_at: r.get(9)?,
        reverted_at: r.get(10)?,
        reverted_by_edit_id: r.get(11)?,
        from_path: r.get(12)?,
    })
}
