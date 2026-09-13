//! The other half of reading: a file that is not text, but is a document whose text can be got
//! out of it (`docs/connectors/filesystem.md` §5).
//!
//! The extraction itself is `gantry-documents`. What lives here is the part that belongs to a
//! session: which of the two a read turned out to be, and the few documents already turned into
//! text, so that reading page 40 of a book does not pay for the first thirty-nine again.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use gantry_documents::Extracted;

use crate::TextFile;

/// How many extracted documents are kept. Small on purpose: this is there to make the second
/// window of one long document cheap, not to hold a library in memory.
const KEPT: usize = 4;

/// What one read returned.
#[derive(Debug, Clone)]
pub enum Reading {
    /// Bytes that are text. Recorded against the session, which is what later makes an edit
    /// possible.
    Text(TextFile),
    /// A document turned into text. Never recorded: the text is not what is on disk, so no edit
    /// may stand on it — and an edit would be refused anyway, since the bytes do not decode.
    Document(Arc<Extracted>),
}

impl Reading {
    /// The text either way, in LF lines.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Text(file) => &file.text,
            Self::Document(doc) => &doc.text,
        }
    }
}

/// The documents this run has already extracted, newest first.
///
/// Keyed by path *and* the hash of the bytes, so a document that changed on disk is extracted
/// again rather than answered from before — the same rule the freshness check uses, for the same
/// reason.
#[derive(Default)]
pub(crate) struct Extractions {
    kept: Mutex<Vec<(PathBuf, String, Arc<Extracted>)>>,
}

impl Extractions {
    pub(crate) fn get(&self, path: &Path, hash: &str) -> Option<Arc<Extracted>> {
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        let at = kept.iter().position(|(p, h, _)| p == path && h == hash)?;
        let entry = kept.remove(at);
        let doc = entry.2.clone();
        kept.insert(0, entry);
        Some(doc)
    }

    pub(crate) fn put(&self, path: &Path, hash: String, doc: Arc<Extracted>) {
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.retain(|(p, _, _)| p != path);
        kept.insert(0, (path.to_path_buf(), hash, doc));
        kept.truncate(KEPT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Arc<Extracted> {
        Arc::new(Extracted {
            kind: gantry_documents::DocumentKind::Pdf,
            text: text.to_owned(),
            page_starts: vec![0],
            pages: 1,
        })
    }

    #[test]
    fn a_document_that_changed_on_disk_is_not_answered_from_before() {
        let kept = Extractions::default();
        let path = Path::new("/docs/a.pdf");
        kept.put(path, "hash-1".to_owned(), doc("first"));
        assert_eq!(kept.get(path, "hash-1").unwrap().text, "first");
        assert!(kept.get(path, "hash-2").is_none());
        kept.put(path, "hash-2".to_owned(), doc("second"));
        assert_eq!(kept.get(path, "hash-2").unwrap().text, "second");
        // One entry per path, so a document read again after a change does not keep the old
        // copy alive alongside the new one.
        assert_eq!(kept.kept.lock().unwrap().len(), 1);
    }

    #[test]
    fn only_the_last_few_are_kept() {
        let kept = Extractions::default();
        for i in 0..KEPT + 2 {
            kept.put(
                &PathBuf::from(format!("/docs/{i}.pdf")),
                "h".to_owned(),
                doc("x"),
            );
        }
        assert_eq!(kept.kept.lock().unwrap().len(), KEPT);
        assert!(kept.get(Path::new("/docs/0.pdf"), "h").is_none());
        assert!(kept.get(Path::new("/docs/5.pdf"), "h").is_some());
    }
}
