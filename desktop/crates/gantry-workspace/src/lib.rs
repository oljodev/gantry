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

pub mod changes;
pub mod document;
pub mod edit;
pub mod journal;
pub mod scope;
pub mod session;
pub mod text;
pub mod walk;

use std::{path::PathBuf, sync::Arc};

use gantry_core::{ChatId, EditOp};
use gantry_store::{BlobStore, Store, repos};

pub use changes::{FileChange, FileDiff, Reverted};
pub use document::Reading;
pub use edit::{Anchor, Change, Diff, EditError, Hunk};
pub use gantry_documents::{DocumentKind, Extracted};
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
    Document(#[from] gantry_documents::DocumentError),
    /// A file whose text can be read but must not be rewritten. Both connectors write text; a
    /// document's bytes are not its text, and writing the one over the other destroys it.
    #[error(
        "{path} is a {kind}. Gantry can read its text but not write it — putting text where the \
         document's bytes are would destroy it. Write to another path instead."
    )]
    NotEditable { path: String, kind: &'static str },
    /// The extraction was taken down with its thread — the app is shutting down, or the task was
    /// aborted. Not a fact about the file, so it says so rather than blaming the document.
    #[error("reading {path} was interrupted before it finished")]
    Interrupted { path: String },
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
    #[error("{path} already exists; move or delete it first, or choose another name")]
    Occupied { path: String },
    #[error("{path} is a folder, not a file: {why}")]
    Directory { path: String, why: String },
    /// Worded for every caller: the editor has nothing to edit, the shell nowhere to run, and
    /// each adds its own next step after this sentence.
    #[error("this chat has no folder attached")]
    NoRoots,
    /// Asked to revert, or to diff, a file this session never changed.
    #[error("this session has not changed {path}, so there is nothing to put back")]
    Unchanged { path: String },
    #[error("{0}")]
    Store(String),
}

/// What a write left behind: the same shape whether it created the file or replaced it.
pub struct Wrote {
    pub edit_id: String,
    pub path: String,
    pub diff: Diff,
    pub created: bool,
}

/// A move or a copy, which is the one operation with two ends.
pub struct Moved {
    pub edit_id: String,
    pub from: String,
    pub to: String,
}

/// A deletion, and which of the two promises it kept.
pub struct Deleted {
    pub edit_id: String,
    pub path: String,
    /// Whether it went to the system trash rather than being removed outright (D7).
    pub trashed: bool,
    /// Set for a directory, which is removed whole and cannot be put back from the journal.
    pub directory: bool,
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
    /// The documents already turned into text.
    extracted: document::Extractions,
    /// Whether deletion may use the system trash. Off in tests, so a test run never puts
    /// anything in the developer's own trash, and available for the platforms where §17's third
    /// open question turns out to have the answer "there is no usable trash here".
    trash: bool,
}

impl Workspace {
    #[must_use]
    pub fn new(store: Arc<Store>, blobs: Arc<BlobStore>, data_dir: PathBuf) -> Self {
        Self {
            journal: Journal::new(store.clone(), blobs),
            store,
            sessions: Sessions::new(),
            denied: vec![data_dir],
            extracted: document::Extractions::default(),
            trash: true,
        }
    }

    /// Turns the system trash off, so deletion removes outright and says so.
    #[must_use]
    pub fn without_trash(mut self) -> Self {
        self.trash = false;
        self
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

    /// Reads a file a reader can put in front of a model: decoded when the bytes are text,
    /// extracted when they are a document Gantry can read (`filesystem.md` §5).
    ///
    /// A text read is remembered, which is what later makes an edit possible. An extraction is
    /// not: the text is not what is on disk, so nothing may be edited on the strength of it.
    pub async fn read(&self, chat: ChatId, scoped: &Scoped<'_>) -> Result<Reading, WorkspaceError> {
        let display = scoped.path.display().to_string();
        let bytes = scoped.read()?;
        // The bytes decide, then the name. A `.pdf` holding nothing but text is read as the text
        // it is, and a PDF saved under any name at all is still extracted.
        let kind = if let Some(kind) = gantry_documents::by_bytes(&bytes) {
            kind
        } else {
            match TextFile::decode(&display, &bytes) {
                Ok(file) => {
                    self.sessions.record(chat, &scoped.path, text::hash(&bytes));
                    return Ok(Reading::Text(file));
                }
                Err(not_text) => gantry_documents::by_name(&display).ok_or(not_text)?,
            }
        };
        let hash = text::hash(&bytes);
        if let Some(hit) = self.extracted.get(&scoped.path, &hash) {
            return Ok(Reading::Document(hit));
        }
        // Seconds, not milliseconds, for a long document: off the runtime's workers, so the rest
        // of the turn — the stream, a Stop — is not waiting behind a book.
        let named = display.clone();
        let doc =
            tokio::task::spawn_blocking(move || gantry_documents::extract(kind, &named, &bytes))
                .await
                .map_err(|_| WorkspaceError::Interrupted {
                    path: display.clone(),
                })??;
        let doc = Arc::new(doc);
        self.extracted.put(&scoped.path, hash, doc.clone());
        Ok(Reading::Document(doc))
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
        let before_bytes = scoped.read()?;
        let file = must_be_text(&display, &before_bytes)?;
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
                from: None,
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

    /// Creates a file or replaces it whole, journaling either way.
    ///
    /// Unlike an edit, this does not require the file to have been read: replacing a file whole
    /// is a deliberate act, and a model that has composed the entire content has not misread
    /// anything. What it will not do is overwrite a file this session read and something else
    /// has changed since — that is the one case where a whole-file write silently destroys work
    /// nobody has seen.
    pub async fn write(
        &self,
        roots: &Roots,
        chat: ChatId,
        call_id: &str,
        path: &str,
        bytes: &[u8],
    ) -> Result<Wrote, WorkspaceError> {
        let scoped = roots.resolve(path)?;
        let display = scoped.path.display().to_string();
        let before = scoped.read().ok();
        // What is already there decides whether this is a write at all: a document, or anything
        // else that is not text, is not this connector's to replace with text. Revert still puts
        // such a file back, because it restores bytes rather than writing text.
        let before_text = before
            .as_deref()
            .map(|bytes| must_be_text(&display, bytes))
            .transpose()?;
        if let Some(before) = &before {
            let current = text::hash(before);
            if self.sessions.freshness(chat, &scoped.path, &current) == Freshness::Changed {
                return Err(WorkspaceError::Stale {
                    path: display,
                    why: "something else has written it since you read it.".to_owned(),
                });
            }
        }
        scoped.create_parent()?;
        scoped.write_atomically(bytes)?;
        self.sessions.record(chat, &scoped.path, text::hash(bytes));

        // A diff only exists when both sides are text. A file created from bytes that are not
        // text — there is no such tool today — would still be journaled, with no hunks to show.
        let diff = match (before_text, TextFile::decode(&display, bytes).ok()) {
            (Some(old), Some(new)) => edit::diff(&old.text, &new.text),
            (None, Some(new)) => edit::diff("", &new.text),
            _ => Diff::default(),
        };
        let edit_id = self
            .journal
            .record(journal::Entry {
                chat_id: chat,
                tool_call_id: call_id,
                path: &display,
                op: if before.is_some() {
                    EditOp::Modify
                } else {
                    EditOp::Create
                },
                before: before.as_deref(),
                after: Some(bytes),
                diff: &diff,
                from: None,
            })
            .await?;
        Ok(Wrote {
            edit_id,
            path: display,
            diff,
            created: before.is_none(),
        })
    }

    /// Creates a folder and every missing folder above it. Journaled with no content, so the
    /// Changes pane shows that the session made it.
    pub async fn create_dir(
        &self,
        roots: &Roots,
        chat: ChatId,
        call_id: &str,
        path: &str,
    ) -> Result<String, WorkspaceError> {
        let scoped = roots.resolve(path)?;
        let display = scoped.path.display().to_string();
        scoped.create_dir_all()?;
        self.journal
            .record(journal::Entry {
                chat_id: chat,
                tool_call_id: call_id,
                path: &display,
                op: EditOp::Create,
                before: None,
                after: None,
                diff: &Diff::default(),
                from: None,
            })
            .await?;
        Ok(display)
    }

    /// Moves or copies inside the chat's folders. Both ends go through every phase of §4, so a
    /// rename cannot be used to carry a file across the boundary in either direction.
    pub async fn transfer(
        &self,
        roots: &Roots,
        chat: ChatId,
        call_id: &str,
        from: &str,
        to: &str,
        copy: bool,
    ) -> Result<Moved, WorkspaceError> {
        let source = roots.resolve(from)?;
        let destination = roots.resolve(to)?;
        let from_display = source.path.display().to_string();
        let to_display = destination.path.display().to_string();
        if destination.exists() {
            return Err(WorkspaceError::Occupied { path: to_display });
        }
        let content = source.read().ok();
        if copy {
            source.copy_to(&destination)?;
        } else {
            source.rename_to(&destination)?;
            self.sessions.forget_path(chat, &source.path);
        }
        if let Some(content) = &content {
            self.sessions
                .record(chat, &destination.path, text::hash(content));
        }
        let edit_id = self
            .journal
            .record(journal::Entry {
                chat_id: chat,
                tool_call_id: call_id,
                path: &to_display,
                op: if copy { EditOp::Create } else { EditOp::Rename },
                before: None,
                after: content.as_deref(),
                diff: &Diff::default(),
                from: Some(&from_display),
            })
            .await?;
        Ok(Moved {
            edit_id,
            from: from_display,
            to: to_display,
        })
    }

    /// Deletes, to the system trash where the platform has one (D7). The result says which of
    /// the two happened, because "deleted" and "moved to the trash" are different promises.
    pub async fn delete(
        &self,
        roots: &Roots,
        chat: ChatId,
        call_id: &str,
        path: &str,
        recursive: bool,
        trash: bool,
    ) -> Result<Deleted, WorkspaceError> {
        let scoped = roots.resolve(path)?;
        let display = scoped.path.display().to_string();
        let stat = scoped.stat()?;
        if stat.is_dir && !recursive {
            return Err(WorkspaceError::Directory {
                path: display,
                why: "deleting a folder needs `recursive` set, and the prompt will say what is \
                      inside it"
                    .to_owned(),
            });
        }
        // A file's content is kept so the journal can put it back; a whole tree is not, and the
        // result says so rather than implying a Revert that would not work.
        let content = (!stat.is_dir).then(|| scoped.read().ok()).flatten();
        let trashed = if trash && self.trash {
            // The trash implementation works on the path rather than on the handle. It is the
            // one operation here that does, and it runs only after every phase of §4 has passed
            // on that exact path.
            trash::delete(&scoped.path).is_ok()
        } else {
            false
        };
        if !trashed {
            if stat.is_dir {
                scoped.remove_dir_all()?;
            } else {
                scoped.remove_file()?;
            }
        }
        self.sessions.forget_path(chat, &scoped.path);
        let edit_id = self
            .journal
            .record(journal::Entry {
                chat_id: chat,
                tool_call_id: call_id,
                path: &display,
                op: EditOp::Delete,
                before: content.as_deref(),
                after: None,
                diff: &Diff::default(),
                from: None,
            })
            .await?;
        Ok(Deleted {
            edit_id,
            path: display,
            trashed,
            directory: stat.is_dir,
        })
    }
}

/// The bytes a write is allowed to stand on. A document Gantry can read is named as one, so the
/// refusal says what the file is rather than that it failed to decode — and because an
/// uncompressed PDF is valid UTF-8, "does it decode?" is not the same question.
fn must_be_text(display: &str, bytes: &[u8]) -> Result<TextFile, WorkspaceError> {
    if let Some(kind) = gantry_documents::by_bytes(bytes) {
        return Err(WorkspaceError::NotEditable {
            path: display.to_owned(),
            kind: kind.label(),
        });
    }
    Ok(TextFile::decode(display, bytes)?)
}
