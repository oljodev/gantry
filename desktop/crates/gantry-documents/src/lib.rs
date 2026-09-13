//! Turning documents into text, so that a file a person thinks of as readable is readable
//! (`docs/connectors/filesystem.md` §5, §17 Q2).
//!
//! Everything here is pure Rust with a permissive licence, because Gantry ships under a
//! commercial one and must not need a C toolchain beyond the one SQLite already costs. That
//! constraint decides the format list rather than ambition: **PDF** is the one binary document
//! with a usable pure-Rust text extractor, and it is also the one that matters most. Everything
//! the roadmap lists alongside it — text, Markdown, code, CSV, JSON — is already text, read by
//! decoding the bytes (`gantry_workspace::TextFile`), and inventing an "extractor" for those
//! would only stand between the model and the file.
//!
//! Two things this crate is careful about, because both are ways to lie to a model:
//!
//! - **A page of images is not an empty document.** A scan has no text layer, and returning
//!   nothing for it reads as "this file is blank". It says what it is instead, and that reading
//!   it would need character recognition Gantry does not have.
//! - **Position is metadata, never a prefix inside the text** (D8). No `--- page 3 ---` markers
//!   in the content; the page each line belongs to is carried alongside it, so a reader can say
//!   which pages its window covered without putting anything in the model's way.

#![forbid(unsafe_code)]

mod pdf;
pub mod sample;

/// A backstop on how much of one document is turned into text. No realistic attachment or
/// knowledge file comes near it; a thousand-page book does, and the result says how many of its
/// pages were read rather than ending without saying so.
pub const MAX_PAGES: usize = 2000;

/// A document Gantry can turn into text. One variant today; each new format is a module here
/// plus a line in [`kind_of`], and nothing else in the app changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    Pdf,
}

impl DocumentKind {
    /// What to call it in a sentence a person or a model reads.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF document",
        }
    }

    #[must_use]
    pub fn mime(self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
        }
    }
}

/// A document's text, and where its pages are in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub kind: DocumentKind,
    /// LF-separated, in reading order, with nothing inserted that was not in the document.
    pub text: String,
    /// The line each page read starts on, zero-based, one entry per page — including pages with
    /// no text, which hold the line their successor starts on.
    pub page_starts: Vec<usize>,
    /// Pages the document has, which is more than were read when it is longer than
    /// [`MAX_PAGES`] or a page part-way through could not be rendered.
    pub pages: usize,
}

impl Extracted {
    #[must_use]
    pub fn pages_read(&self) -> usize {
        self.page_starts.len()
    }

    /// Whether every page is in [`Self::text`].
    #[must_use]
    pub fn whole(&self) -> bool {
        self.pages_read() == self.pages
    }

    /// Which pages a window of lines covers, both ends one-based and inclusive, taking the same
    /// one-based line numbers a reader reports. `None` for a document with no pages at all.
    #[must_use]
    pub fn pages_for(&self, first_line: usize, last_line: usize) -> Option<(usize, usize)> {
        if self.page_starts.is_empty() {
            return None;
        }
        let page_of = |line: usize| {
            let zero_based = line.saturating_sub(1);
            // The last page that starts at or before this line. Ties go to the later page,
            // which is what makes a page with no text sit before its successor's text rather
            // than claiming it.
            self.page_starts
                .partition_point(|&start| start <= zero_based)
                .max(1)
        };
        Some((page_of(first_line), page_of(last_line)))
    }
}

/// Why a document Gantry recognised could not be read. Each message says what to do next,
/// because the model reads it as a tool result and acts on it (`filesystem.md` §11).
#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error(
        "{path} is a {kind} Gantry could not read: {why}. It may be damaged or use a feature \
         Gantry does not support; nothing else here will read it either."
    )]
    Unreadable {
        path: String,
        kind: &'static str,
        why: String,
    },
    #[error(
        "{path} is a password-protected {kind}, and Gantry has no password for it. An \
         unprotected copy can be read."
    )]
    Protected { path: String, kind: &'static str },
    #[error(
        "{path} is a {kind} of {pages} page(s) with no text in it, so it is a scan or a \
         drawing rather than a document. Reading it would need character recognition, which \
         Gantry does not have."
    )]
    NoText {
        path: String,
        kind: &'static str,
        pages: usize,
    },
}

/// Which document these bytes are, from the bytes alone, or `None` when they are not one Gantry
/// can read.
///
/// The bytes are asked first everywhere, because an extension is a claim and the content is the
/// fact: a `.pdf` holding nothing but text is read as the text it is, and a PDF saved as `.dat`
/// is still read.
#[must_use]
pub fn by_bytes(bytes: &[u8]) -> Option<DocumentKind> {
    // A PDF's signature is allowed to sit a little way into the file, and readers in the wild
    // accept that, so one behind a stray byte-order mark is still a PDF.
    let head = &bytes[..bytes.len().min(1024)];
    if head.windows(5).any(|w| w == b"%PDF-") {
        return Some(DocumentKind::Pdf);
    }
    None
}

/// Which document a name claims, for bytes that are not text and carry no signature Gantry
/// knows — the last guess before refusing them as binary.
#[must_use]
pub fn by_name(name: &str) -> Option<DocumentKind> {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => Some(DocumentKind::Pdf),
        _ => None,
    }
}

/// [`by_bytes`], then [`by_name`]: for a caller holding both and no way to tell text from binary
/// itself.
#[must_use]
pub fn kind_of(name: &str, bytes: &[u8]) -> Option<DocumentKind> {
    by_bytes(bytes).or_else(|| by_name(name))
}

/// Extracts the text of a document. `path` is only ever put into messages.
///
/// Synchronous and CPU-bound: a long document costs seconds, not milliseconds, so a caller on an
/// async task hands this to a blocking thread rather than to a runtime worker —
/// `gantry_workspace::Workspace::interpret` is the one that does.
pub fn extract(kind: DocumentKind, path: &str, bytes: &[u8]) -> Result<Extracted, DocumentError> {
    let pages = match kind {
        DocumentKind::Pdf => pdf::pages(path, bytes)?,
    };
    let mut lines: Vec<&str> = Vec::new();
    let mut page_starts = Vec::with_capacity(pages.texts.len());
    let mut tidied = Vec::with_capacity(pages.texts.len());
    for page in &pages.texts {
        tidied.push(tidy(page));
    }
    for page in &tidied {
        if !lines.is_empty() && !page.is_empty() {
            lines.push("");
        }
        page_starts.push(lines.len());
        lines.extend(page.lines());
    }
    let text = lines.join("\n");
    if text.trim().is_empty() {
        return Err(DocumentError::NoText {
            path: path.to_owned(),
            kind: kind.label(),
            pages: pages.total,
        });
    }
    Ok(Extracted {
        kind,
        text,
        page_starts,
        pages: pages.total,
    })
}

/// One page's text as text: LF line endings, no trailing spaces, no blank run at either end.
/// A PDF holds glyphs at positions rather than lines, so an extractor's idea of where a line
/// ends is approximate and its idea of trailing space is meaningless.
fn tidy(page: &str) -> String {
    let page = page.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(page.len());
    for line in page.lines() {
        let line = line.trim_end();
        if line.is_empty() && out.ends_with("\n\n") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::{sample::pdf, *};

    #[test]
    fn a_pdf_is_recognised_by_its_bytes_and_by_its_name() {
        assert_eq!(by_bytes(&pdf(&["hi"])), Some(DocumentKind::Pdf));
        assert_eq!(kind_of("x.bin", &pdf(&["hi"])), Some(DocumentKind::Pdf));
        // The name is the fallback, never the first word: a file called `.pdf` that holds text
        // carries no signature, so the bytes alone say nothing about it.
        assert_eq!(by_bytes(b"just text"), None);
        assert_eq!(kind_of("report.PDF", b"just text"), Some(DocumentKind::Pdf));
        assert_eq!(kind_of("notes.md", b"# hi"), None);
        assert_eq!(kind_of("", b""), None);
    }

    #[test]
    fn the_text_of_every_page_comes_out_in_order() {
        let bytes = pdf(&["Gantry reads documents", "Second page here"]);
        let out = extract(DocumentKind::Pdf, "/docs/two.pdf", &bytes).unwrap();
        assert!(
            out.text.contains("Gantry reads documents"),
            "{:?}",
            out.text
        );
        assert!(out.text.contains("Second page here"), "{:?}", out.text);
        assert_eq!((out.pages, out.pages_read()), (2, 2));
        assert!(out.whole());
        assert!(
            out.text.find("Gantry") < out.text.find("Second"),
            "{:?}",
            out.text
        );
    }

    #[test]
    fn a_window_of_lines_knows_which_pages_it_covered() {
        let bytes = pdf(&["one\ntwo", "three"]);
        let out = extract(DocumentKind::Pdf, "/docs/three.pdf", &bytes).unwrap();
        assert_eq!(out.pages, 2);
        assert_eq!(out.pages_for(1, 1), Some((1, 1)));
        assert_eq!(out.pages_for(1, out.text.lines().count()), Some((1, 2)));
        // Nothing has page zero, and a line past the end belongs to the last page rather than
        // to a page that does not exist.
        assert_eq!(out.pages_for(0, 900), Some((1, 2)));
    }

    #[test]
    fn a_page_of_images_says_so_rather_than_coming_back_empty() {
        let err = extract(DocumentKind::Pdf, "/docs/scan.pdf", &pdf(&["", ""])).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("2 page(s) with no text"), "{message}");
        assert!(message.contains("character recognition"), "{message}");
    }

    #[test]
    fn a_damaged_pdf_is_refused_rather_than_taking_the_process_with_it() {
        let bytes = pdf(&["readable"]);
        let truncated = &bytes[..bytes.len() / 2];
        let err = extract(DocumentKind::Pdf, "/docs/broken.pdf", truncated).unwrap_err();
        assert!(err.to_string().contains("could not read"), "{err}");
        let err = extract(DocumentKind::Pdf, "/docs/empty.pdf", b"%PDF-1.4\n").unwrap_err();
        assert!(err.to_string().contains("could not read"), "{err}");
    }

    #[test]
    fn blank_runs_inside_a_page_collapse_but_the_words_are_untouched() {
        assert_eq!(tidy("a   \n\n\n\nb\n"), "a\n\nb");
        assert_eq!(tidy("\n\n  keep  me\r\n"), "keep  me");
    }
}
