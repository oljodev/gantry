//! The edit journal (`docs/connectors/code-editor.md` §4).
//!
//! Every change writes one row and both versions of the file. That is what the Changes pane
//! lists, what the diff drawer renders, what per-file and whole-session **Revert** replay, and
//! what `undo` reads. A call that fails writes no row; a call that succeeds writes exactly one,
//! even when the patch touched five places.

use std::sync::Arc;

use gantry_core::{ChatId, EditOp, now_ms};
use gantry_store::{
    BlobStore, Store,
    repos::{blobs, file_edits},
};

use crate::edit::Diff;

pub use gantry_store::repos::file_edits::FileEditRecord;

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("the edit could not be recorded: {0}")]
    Store(String),
}

/// One change, as it goes into the journal.
pub struct Entry<'a> {
    pub chat_id: ChatId,
    pub tool_call_id: &'a str,
    pub path: &'a str,
    pub op: EditOp,
    /// Absent when the file did not exist before.
    pub before: Option<&'a [u8]>,
    /// Absent when the file was deleted.
    pub after: Option<&'a [u8]>,
    pub diff: &'a Diff,
}

pub struct Journal {
    store: Arc<Store>,
    blobs: Arc<BlobStore>,
}

impl Journal {
    #[must_use]
    pub fn new(store: Arc<Store>, blobs: Arc<BlobStore>) -> Self {
        Self { store, blobs }
    }

    /// Stores both versions and the row. Returns the edit's id.
    pub async fn record(&self, entry: Entry<'_>) -> Result<String, JournalError> {
        let before = self.put(entry.before)?;
        let after = self.put(entry.after)?;
        let record = file_edits::FileEditRecord {
            id: ulid_string(),
            tool_call_id: entry.tool_call_id.to_owned(),
            chat_id: entry.chat_id,
            path: entry.path.to_owned(),
            op: entry.op,
            before_blob_hash: before.as_ref().map(|(h, _)| h.clone()),
            after_blob_hash: after.as_ref().map(|(h, _)| h.clone()),
            hunks_json: serde_json::to_string(&entry.diff.hunks).unwrap_or_else(|_| "[]".into()),
            stats_json: format!(
                "{{\"added\":{},\"removed\":{}}}",
                entry.diff.added, entry.diff.removed
            ),
            applied_at: now_ms(),
            reverted_at: None,
            reverted_by_edit_id: None,
        };
        let id = record.id.clone();
        let sizes: Vec<(String, i64)> = before.into_iter().chain(after).collect();
        self.store
            .write(move |conn| {
                let tx = conn.transaction()?;
                for (hash, size) in &sizes {
                    blobs::add_ref(&tx, hash, *size, Some("text/plain"))?;
                }
                file_edits::insert(&tx, &record)?;
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| JournalError::Store(e.to_string()))?;
        Ok(id)
    }

    /// This chat's edits to one file, oldest first.
    pub fn history(&self, chat: ChatId, path: &str) -> Result<Vec<FileEditRecord>, JournalError> {
        let path = path.to_owned();
        self.store
            .read(|conn| file_edits::for_path(conn, chat, &path))
            .map_err(|e| JournalError::Store(e.to_string()))
    }

    /// This chat's edits to every file, oldest first: the Changes pane.
    pub fn for_chat(&self, chat: ChatId) -> Result<Vec<FileEditRecord>, JournalError> {
        self.store
            .read(|conn| file_edits::for_chat(conn, chat))
            .map_err(|e| JournalError::Store(e.to_string()))
    }

    /// The stored bytes of a version.
    pub fn content(&self, hash: &str) -> Result<Vec<u8>, JournalError> {
        self.blobs
            .get(hash)
            .map_err(|e| JournalError::Store(e.to_string()))
    }

    /// Marks edits undone. A revert never deletes a row (§4).
    pub async fn mark_reverted(&self, ids: Vec<String>, by: String) -> Result<(), JournalError> {
        self.store
            .write(move |conn| file_edits::mark_reverted(conn, &ids, &by))
            .await
            .map_err(|e| JournalError::Store(e.to_string()))
    }

    fn put(&self, bytes: Option<&[u8]>) -> Result<Option<(String, i64)>, JournalError> {
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        let hash = self
            .blobs
            .put(bytes)
            .map_err(|e| JournalError::Store(e.to_string()))?;
        Ok(Some((hash, bytes.len() as i64)))
    }
}

/// The journal's own ids. Lexicographically ordered by time, like every other id in Gantry.
fn ulid_string() -> String {
    ulid::Ulid::new().to_string()
}
