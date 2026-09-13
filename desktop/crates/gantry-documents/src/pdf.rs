//! PDF text, one string per page.
//!
//! `pdf-extract` is the choice `filesystem.md` §13 left to build time: pure Rust, MIT, no C
//! toolchain, and the only crate in that set carrying the font tables a PDF needs before its
//! bytes mean anything — a PDF stores glyph codes in each font's own encoding, so "pull the
//! strings out of the content stream" produces mojibake for every document that is not plain
//! WinAnsi Helvetica.
//!
//! The pages are walked here rather than through its one-call helper for three reasons: the page
//! count is known before the text is (so a document longer than [`MAX_PAGES`] can say what it
//! left out instead of pretending it ended), a password-protected file is recognised as such
//! rather than guessed at from an error string, and a page that fails to render stops the walk
//! instead of silently emptying the whole result.
//!
//! What the library does not offer is robustness: it panics on a number of malformed files
//! rather than returning an error. A corrupt PDF in a folder the user attached is an ordinary
//! thing to meet and must not take the app down, so the walk runs inside `catch_unwind`. The
//! panic's own message still reaches stderr through the default hook; replacing that hook is
//! process-wide, and losing another thread's panic message to tidy up our own is a bad trade.

use lopdf::Document;
use pdf_extract::{PlainTextOutput, output_doc_page};

use crate::{DocumentError, DocumentKind, MAX_PAGES};

/// What one document gave up: the pages that were read, and how many it has.
pub(crate) struct Pages {
    pub texts: Vec<String>,
    pub total: usize,
}

/// Reads up to [`MAX_PAGES`] pages. `path` is only ever put into messages.
pub(crate) fn pages(path: &str, bytes: &[u8]) -> Result<Pages, DocumentError> {
    let kind = DocumentKind::Pdf.label();
    let unreadable = |why: String| DocumentError::Unreadable {
        path: path.to_owned(),
        kind,
        why,
    };
    match std::panic::catch_unwind(|| walk(bytes)) {
        Ok(Ok(pages)) if pages.texts.is_empty() => {
            Err(unreadable("none of its pages could be parsed".to_owned()))
        }
        Ok(Ok(pages)) => Ok(pages),
        Ok(Err(Failure::Protected)) => Err(DocumentError::Protected {
            path: path.to_owned(),
            kind,
        }),
        Ok(Err(Failure::Broken(why))) => {
            log::debug!("pdf extraction failed for {path}: {why}");
            Err(unreadable(why))
        }
        Err(_) => {
            log::warn!("pdf extraction panicked on {path}");
            Err(unreadable("its structure is damaged".to_owned()))
        }
    }
}

enum Failure {
    Protected,
    Broken(String),
}

fn walk(bytes: &[u8]) -> Result<Pages, Failure> {
    let mut doc = Document::load_mem(bytes).map_err(|err| Failure::Broken(err.to_string()))?;
    // An owner password with no user password opens on an empty one, which is most of what
    // "protected" means in the wild; only a real user password is refused.
    if doc.is_encrypted() && doc.decrypt("").is_err() {
        return Err(Failure::Protected);
    }
    let total = doc.get_pages().len();
    let mut texts = Vec::with_capacity(total.min(MAX_PAGES));
    for page in 1..=u32::try_from(total.min(MAX_PAGES)).unwrap_or(u32::MAX) {
        let mut text = String::new();
        // The writer borrows the string, so it lives in a block of its own.
        let rendered = {
            let mut out = PlainTextOutput::new(&mut text);
            output_doc_page(&doc, &mut out, page)
        };
        if let Err(err) = rendered {
            log::debug!("pdf page {page} of {total} failed: {err}");
            break;
        }
        texts.push(text);
    }
    Ok(Pages { texts, total })
}
