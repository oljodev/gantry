//! What this chat has read, and whether it is still true.
//!
//! The single highest-volume recoverable failure in agent file editing is acting on a file that
//! has changed since it was read (`docs/connectors/filesystem.md` §11). Gantry treats it as a
//! first-class condition rather than an I/O error: every read records the bytes' hash, every
//! write updates it, and every change re-hashes the file on disk before touching it.
//!
//! The table is in memory and per chat, which is what "in this session" means. A restart
//! forgets, and the model is told to read again — the safe direction.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::RwLock,
};

use gantry_core::ChatId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Never read in this session. The model is told which tool reads it.
    Unseen,
    /// The bytes are the ones this session last saw.
    Unchanged,
    /// Something else wrote the file — usually the user's own editor.
    Changed,
}

#[derive(Debug, Default)]
pub struct Sessions {
    seen: RwLock<HashMap<ChatId, HashMap<PathBuf, String>>>,
}

impl Sessions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Remembers what a read (or a write) saw.
    pub fn record(&self, chat: ChatId, path: &Path, hash: String) {
        self.seen
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .entry(chat)
            .or_default()
            .insert(path.to_path_buf(), hash);
    }

    #[must_use]
    pub fn freshness(&self, chat: ChatId, path: &Path, current: &str) -> Freshness {
        match self
            .seen
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&chat)
            .and_then(|files| files.get(path))
        {
            None => Freshness::Unseen,
            Some(hash) if hash == current => Freshness::Unchanged,
            Some(_) => Freshness::Changed,
        }
    }

    /// Forgets one file: it was moved or deleted, and what this session read of it is no
    /// longer true of anything on disk.
    pub fn forget_path(&self, chat: ChatId, path: &Path) {
        if let Some(files) = self
            .seen
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&chat)
        {
            files.remove(path);
        }
    }

    /// Drops a chat's table: the chat was deleted, or its folders changed.
    pub fn forget(&self, chat: ChatId) {
        self.seen
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&chat);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_unseen_then_unchanged_then_changed() {
        let sessions = Sessions::new();
        let chat = ChatId::new();
        let path = Path::new("/w/a.rs");
        assert_eq!(sessions.freshness(chat, path, "aa"), Freshness::Unseen);
        sessions.record(chat, path, "aa".to_owned());
        assert_eq!(sessions.freshness(chat, path, "aa"), Freshness::Unchanged);
        assert_eq!(sessions.freshness(chat, path, "bb"), Freshness::Changed);
        // Another chat never inherits what this one read.
        assert_eq!(
            sessions.freshness(ChatId::new(), path, "aa"),
            Freshness::Unseen
        );
    }
}
