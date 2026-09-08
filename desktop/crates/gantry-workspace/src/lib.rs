//! The layer the three local connectors sit on: which folders a chat may touch, how a file is
//! read and written without changing what nobody asked to change, what this session has seen,
//! and the journal that makes every change revertible.
//!
//! It exists so that containment is implemented once. Three implementations of a scope check
//! is three chances to get it wrong (`docs/connectors/filesystem.md` D1), and the published
//! failures in that document are all one missing line in one of them. `filesystem` reads and
//! writes whole files, `code-editor` changes them in place, `shell` runs commands in them;
//! none of the three decides for itself where the boundary is.

#![forbid(unsafe_code)]

pub mod edit;
pub mod guard;
pub mod journal;
pub mod scope;
pub mod session;
pub mod text;

use std::{path::PathBuf, sync::Arc};

use gantry_core::{ChatId, EditOp};
use gantry_store::{BlobStore, Store, repos};

pub use edit::{Anchor, Change, Diff, EditError, Hunk};
pub use journal::{FileEditRecord, Journal, JournalError};
pub use scope::{Roots, ScopeError, Scoped};
pub use session::{Freshness, Sessions};
pub use text::{TextError, TextFile};

/// The plan document that specifies this crate.
pub const PLAN_DOCUMENT: &str = "docs/plan/03-connector-system.md";

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("{0}")]
    Scope(#[from] ScopeError),
    #[error("{0}")]
    Text(#[from] TextError),
    #[error("{0}")]
    Edit(#[from] EditError),
    #[error("{0}")]
    Journal(#[from] JournalError),
    /// The rule of §5 the model most often trips: editing something it never read.
    #[error(
        "{path} has not been read in this session. Read it with filesystem__read_file first, \
         then edit the passage exactly as it appears there."
    )]
    Unseen { path: String },
    /// The file moved underneath the edit and the passage no longer fits (§5).
    #[error(
        "{path} changed on disk since it was read, and the edit no longer fits what is there: \
         {why} Read the file again."
    )]
    Stale { path: String, why: String },
    #[error(
        "{path} holds credentials ({why}); Gantry does not let a model edit it. Change it yourself."
    )]
    Sensitive { path: String, why: &'static str },
    #[error("this chat has no folder attached, so there is nothing to edit")]
    NoRoots,
    #[error("{0}")]
    Store(String),
}

/// A file as it was read: the text, how it is spelled on disk, and where it lives.
pub struct ReadFile<'a> {
    pub scoped: Scoped<'a>,
    pub file: TextFile,
    pub bytes: Vec<u8>,
}

/// What one applied change did.
pub struct Applied {
    pub edit_id: String,
    pub path: String,
    pub diff: Diff,
    /// Set when something worth saying happened alongside the edit — the file had changed
    /// underneath, and the change was elsewhere in it.
    pub note: Option<String>,
}

/// Roots, reads, writes and the journal, shared by every local connector.
pub struct Workspace {
    store: Arc<Store>,
    journal: Journal,
    sessions: Sessions,
    /// Gantry's own data, which is never writable whatever the roots say (D6).
    denied: Vec<PathBuf>,
}

impl Workspace {
    #[must_use]
    pub fn new(store: Arc<Store>, blobs: Arc<BlobStore>, data_dir: PathBuf) -> Self {
        Self {
            journal: Journal::new(store.clone(), blobs),
            store,
            sessions: Sessions::new(),
            denied: vec![data_dir],
        }
    }

    #[must_use]
    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    #[must_use]
    pub fn sessions(&self) -> &Sessions {
        &self.sessions
    }

    /// The folders attached to a chat, opened as handles for this call and no longer (D12).
    pub fn roots(&self, chat: ChatId) -> Result<Roots, WorkspaceError> {
        let paths = self
            .store
            .read(move |conn| repos::chats::roots(conn, chat))
            .map_err(|e| WorkspaceError::Store(e.to_string()))?;
        if paths.is_empty() {
            return Err(WorkspaceError::NoRoots);
        }
        let roots = Roots::open(&paths, &self.denied);
        if roots.is_empty() {
            return Err(WorkspaceError::NoRoots);
        }
        Ok(roots)
    }

    /// Reads a file and remembers what it saw, which is what later makes an edit possible.
    pub fn read<'a>(
        &self,
        roots: &'a Roots,
        chat: ChatId,
        path: &str,
    ) -> Result<ReadFile<'a>, WorkspaceError> {
        let scoped = roots.resolve(path)?;
        let bytes = scoped.read()?;
        let file = TextFile::decode(&scoped.path.display().to_string(), &bytes)?;
        self.sessions.record(chat, &scoped.path, text::hash(&bytes));
        Ok(ReadFile {
            scoped,
            file,
            bytes,
        })
    }

    /// Applies one change: the freshness rules of `code-editor.md` §5, the change itself, an
    /// atomic write, and one journal row. Nothing is written unless every step succeeded.
    pub async fn apply(
        &self,
        roots: &Roots,
        chat: ChatId,
        call_id: &str,
        path: &str,
        change: &Change,
    ) -> Result<Applied, WorkspaceError> {
        let scoped = roots.resolve(path)?;
        let display = scoped.path.display().to_string();
        if let Some(why) = guard::sensitive(&scoped.path) {
            return Err(WorkspaceError::Sensitive { path: display, why });
        }
        let before_bytes = scoped.read()?;
        let file = TextFile::decode(&display, &before_bytes)?;
        let current = text::hash(&before_bytes);
        let freshness = self.sessions.freshness(chat, &scoped.path, &current);
        if freshness == Freshness::Unseen {
            return Err(WorkspaceError::Unseen { path: display });
        }
        // A file that moved underneath may still take the edit, when the passage is still
        // there exactly once and the change was elsewhere. A line number cannot survive that,
        // so an `at_line` insert is refused rather than guessed at.
        if freshness == Freshness::Changed
            && matches!(
                change,
                Change::Insert {
                    anchor: Anchor::AtLine(_),
                    ..
                }
            )
        {
            return Err(WorkspaceError::Stale {
                path: display,
                why: "line numbers no longer mean what they did when you read it.".to_owned(),
            });
        }
        let after_text = match edit::apply(&file.text, change) {
            Ok(text) => text,
            Err(err) if freshness == Freshness::Changed => {
                return Err(WorkspaceError::Stale {
                    path: display,
                    why: err.to_string(),
                });
            }
            Err(err) => return Err(err.into()),
        };

        let after_bytes = file.encode(&after_text);
        scoped.write_atomically(&after_bytes)?;
        self.sessions
            .record(chat, &scoped.path, text::hash(&after_bytes));

        let diff = edit::diff(&file.text, &after_text);
        let edit_id = self
            .journal
            .record(journal::Entry {
                chat_id: chat,
                tool_call_id: call_id,
                path: &display,
                op: EditOp::Modify,
                before: Some(&before_bytes),
                after: Some(&after_bytes),
                diff: &diff,
            })
            .await?;
        Ok(Applied {
            edit_id,
            path: display,
            diff,
            note: (freshness == Freshness::Changed).then(|| {
                "the file had changed on disk since it was read; the change was elsewhere in it \
                 and the edit still applied"
                    .to_owned()
            }),
        })
    }
}
