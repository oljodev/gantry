//! What a session did to the folder, and putting it back (`docs/plan/16 §5`).
//!
//! The journal already holds every change and both versions of every file. This is the reading
//! of it a person wants during a coding session — *what has it actually done to my repository* —
//! and the reverse of it: restore the file to what it was before this session touched it.
//!
//! Three rules shape the whole module.
//!
//! **Net, not per-edit**: a file edited five times is one row with one diff, from what it was
//! when the session started to what it is now, because five separate hunks of the same function
//! is not the question anyone is asking.
//!
//! **A file that is back is not a change.** The comparison is the file at the session's first
//! edit against the file now, so a file the model edited and then edited back drops off the
//! list, and so does one that has been reverted — the revert is itself a journal row, and a list
//! built from open rows would show it as new work.
//!
//! **A revert never guesses**: if the file on disk is not what Gantry last left there, something
//! else has written it, and putting the old bytes back would throw that work away — so it
//! refuses and says so, exactly as `undo` does.

use std::collections::BTreeMap;

use gantry_core::{ChatId, EditOp};
use gantry_store::repos::file_edits::FileEditRecord;

use crate::{
    Roots, Workspace, WorkspaceError,
    edit::{self, Diff},
    journal,
    text::{self, TextFile},
};

/// One file this session changed, as the Changes pane lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// Absolute, as the journal records it.
    pub path: String,
    /// What the session did to the file overall: created it, changed it, or removed it.
    pub op: EditOp,
    pub added: usize,
    pub removed: usize,
    /// How many separate edits went into that.
    pub edits: usize,
    /// When the last of them landed.
    pub last_at: i64,
    /// A file whose versions are not text: counted and revertible, but not shown as a diff.
    pub binary: bool,
}

/// A file's whole session diff, for the pane below the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub op: EditOp,
    pub diff: Diff,
    pub binary: bool,
}

/// What a revert put back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reverted {
    pub path: String,
    /// What the revert itself did: `delete` when the session had created the file.
    pub op: EditOp,
    pub added: usize,
    pub removed: usize,
    /// How many journal rows it closed.
    pub edits: usize,
}

impl Workspace {
    /// Every file this session has changed and not yet put back, most recently changed first.
    pub fn changes(&self, chat: ChatId) -> Result<Vec<FileChange>, WorkspaceError> {
        let mut out = Vec::new();
        for (path, edits) in self.by_file(chat)? {
            if !changed(&edits) {
                continue;
            }
            let (op, diff, binary) = self.net(&edits)?;
            out.push(FileChange {
                path,
                op,
                added: diff.added,
                removed: diff.removed,
                edits: edits.iter().filter(|e| e.reverted_at.is_none()).count(),
                last_at: edits.last().map_or(0, |e| e.applied_at),
                binary,
            });
        }
        out.sort_by(|a, b| b.last_at.cmp(&a.last_at).then_with(|| a.path.cmp(&b.path)));
        Ok(out)
    }

    /// One file's session diff. A path this session never changed is an error rather than an
    /// empty diff: the pane only ever asks about a file its own list gave it.
    pub fn file_change(&self, chat: ChatId, path: &str) -> Result<FileDiff, WorkspaceError> {
        let edits = self.history_for(chat, path)?;
        let (op, diff, binary) = self.net(&edits)?;
        Ok(FileDiff {
            path: path.to_owned(),
            op,
            diff,
            binary,
        })
    }

    /// Puts one file back to what it was before this session touched it.
    ///
    /// The restore is journalled like any other change, so it is itself in the history and the
    /// rows it closed keep their place — a revert never deletes a row (`code-editor.md` §4).
    pub async fn revert_file(
        &self,
        roots: &Roots,
        chat: ChatId,
        path: &str,
    ) -> Result<Reverted, WorkspaceError> {
        let scoped = roots.resolve(path)?;
        let display = scoped.path.display().to_string();
        let edits = self.history_for(chat, &display)?;
        if !changed(&edits) {
            return Err(WorkspaceError::Unchanged { path: display });
        }
        let newest = edits.last().expect("history_for refuses an empty history");
        let origin = edits.first().expect("history_for refuses an empty history");

        // What is on disk has to be what Gantry last left there. Anything else and the file has
        // been written by something we cannot see, and the old bytes would erase it.
        let current = scoped.read().ok();
        let untouched = match (&newest.after_blob_hash, &current) {
            (Some(hash), Some(bytes)) => text::hash(bytes) == *hash,
            (None, None) => true,
            _ => false,
        };
        if !untouched {
            return Err(WorkspaceError::Stale {
                path: display,
                why: "it has changed since Gantry last wrote it, and reverting would discard \
                      that change."
                    .to_owned(),
            });
        }

        // Every row still in force is closed by this one revert, including a revert of its own
        // that a later edit undid.
        let ids: Vec<String> = edits
            .iter()
            .filter(|e| e.reverted_at.is_none())
            .map(|e| e.id.clone())
            .collect();
        let closed = ids.len();

        // No version to go back to means the session created the file, so putting it back means
        // removing it. It goes to the trash where the platform has one: the user asked to undo
        // a creation, not to lose whatever they have since put in it.
        let Some(hash) = origin.before_blob_hash.as_deref() else {
            let call_id = revert_call_id();
            let removed = current.as_ref().map_or(0, |bytes| {
                TextFile::decode(&display, bytes).map_or(0, |file| file.text.lines().count())
            });
            let deleted = self
                .delete(roots, chat, &call_id, &display, false, true)
                .await?;
            self.journal.mark_reverted(ids, deleted.edit_id).await?;
            return Ok(Reverted {
                path: display,
                op: EditOp::Delete,
                added: 0,
                removed,
                edits: closed,
            });
        };

        let restored = self.journal.content(hash)?;
        let before_bytes = current.unwrap_or_default();
        scoped.create_parent()?;
        scoped.write_atomically(&restored)?;
        // The session has now seen this file in this state; without saying so the next edit
        // would call it stale (`filesystem.md` §5).
        self.sessions
            .record(chat, &scoped.path, text::hash(&restored));

        let diff = match (
            TextFile::decode(&display, &before_bytes),
            TextFile::decode(&display, &restored),
        ) {
            (Ok(before), Ok(after)) => edit::diff(&before.text, &after.text),
            _ => Diff::default(),
        };
        let edit_id = self
            .journal
            .record(journal::Entry {
                chat_id: chat,
                tool_call_id: &revert_call_id(),
                path: &display,
                op: if before_bytes.is_empty() {
                    EditOp::Create
                } else {
                    EditOp::Modify
                },
                before: (!before_bytes.is_empty()).then_some(before_bytes.as_slice()),
                after: Some(&restored),
                diff: &diff,
                from: None,
            })
            .await?;
        self.journal.mark_reverted(ids, edit_id).await?;
        Ok(Reverted {
            path: display,
            op: EditOp::Modify,
            added: diff.added,
            removed: diff.removed,
            edits: closed,
        })
    }

    /// This session's edits, by file, oldest first within each — reverted rows included,
    /// because the file's state at the session's start is the first row's `before`, whatever
    /// happened to it since.
    fn by_file(
        &self,
        chat: ChatId,
    ) -> Result<BTreeMap<String, Vec<FileEditRecord>>, WorkspaceError> {
        let mut by_path: BTreeMap<String, Vec<FileEditRecord>> = BTreeMap::new();
        for edit in self.journal.for_chat(chat)? {
            by_path.entry(edit.path.clone()).or_default().push(edit);
        }
        Ok(by_path)
    }

    fn history_for(&self, chat: ChatId, path: &str) -> Result<Vec<FileEditRecord>, WorkspaceError> {
        let edits = self.journal.history(chat, path)?;
        if edits.is_empty() {
            return Err(WorkspaceError::Unchanged {
                path: path.to_owned(),
            });
        }
        Ok(edits)
    }

    /// The diff from what the file was before the first of these edits to what it is after the
    /// last, which is the only version of the story anyone wants.
    fn net(&self, edits: &[FileEditRecord]) -> Result<(EditOp, Diff, bool), WorkspaceError> {
        let (Some(first), Some(last)) = (edits.first(), edits.last()) else {
            return Ok((EditOp::Modify, Diff::default(), false));
        };
        let op = match (&first.before_blob_hash, &last.after_blob_hash) {
            (None, Some(_)) => EditOp::Create,
            (Some(_), None) => EditOp::Delete,
            _ => EditOp::Modify,
        };
        let before = self.version(first.before_blob_hash.as_deref(), &first.path)?;
        let after = self.version(last.after_blob_hash.as_deref(), &last.path)?;
        match (before, after) {
            (Some(before), Some(after)) => Ok((op, edit::diff(&before, &after), false)),
            // One end of the story is not text. It is still a change, still counted as one file
            // and still revertible; it is only the diff that cannot be drawn.
            _ => Ok((op, Diff::default(), true)),
        }
    }

    /// A stored version as text, or `None` when it is not text. An absent hash is the empty
    /// file, which is what a creation starts from and a deletion ends at.
    fn version(&self, hash: Option<&str>, path: &str) -> Result<Option<String>, WorkspaceError> {
        let Some(hash) = hash else {
            return Ok(Some(String::new()));
        };
        let bytes = self.journal.content(hash)?;
        Ok(TextFile::decode(path, &bytes).ok().map(|file| file.text))
    }
}

/// Whether the file is anywhere other than where the session found it. Comparing the two ends
/// of the history is exact and cheap — the hashes are already in the rows — and it is the same
/// answer for a text file and a binary one.
fn changed(edits: &[FileEditRecord]) -> bool {
    match (edits.first(), edits.last()) {
        (Some(first), Some(last)) => first.before_blob_hash != last.after_blob_hash,
        _ => false,
    }
}

/// The journal wants a call id, and a revert is not a tool call. It gets one of its own, so the
/// history says plainly which rows the person made and which the model did.
fn revert_call_id() -> String {
    format!("revert-{}", ulid::Ulid::new())
}
