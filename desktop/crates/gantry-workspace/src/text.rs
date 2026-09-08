//! Reading and writing text without changing anything the user did not ask to change.
//!
//! A tool that quietly rewrites a CRLF file as LF produces a diff touching every line, which is
//! both useless to review and a real way to lose work (`docs/connectors/filesystem.md` §5). So
//! what reading detects — byte-order mark, line ending, whether the file ended with a newline —
//! is carried on the value and put back on write. Inside Gantry the text is always LF, which is
//! what makes a model's `old` string match a file it has never seen the line endings of.

use sha2::{Digest, Sha256};

/// How many bytes decide whether a file is text at all.
const SNIFF: usize = 8192;

#[derive(Debug, thiserror::Error)]
pub enum TextError {
    #[error("{path} is not a text file ({kind}, {size} bytes); this connector only edits text")]
    NotText {
        path: String,
        kind: &'static str,
        size: usize,
    },
}

/// A file's text, plus everything about its spelling that a write has to preserve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFile {
    /// Always LF-separated, with no byte-order mark: the form everything else works in.
    pub text: String,
    pub bom: bool,
    pub crlf: bool,
    /// Whether the file on disk ended with a newline. An editor that adds one silently is as
    /// annoying as one that drops it.
    pub final_newline: bool,
}

impl TextFile {
    /// Decodes bytes, or explains honestly why they are not text (D10: never decoded bytes).
    pub fn decode(path: &str, bytes: &[u8]) -> Result<Self, TextError> {
        let not_text = |kind| {
            Err(TextError::NotText {
                path: path.to_owned(),
                kind,
                size: bytes.len(),
            })
        };
        let head = &bytes[..bytes.len().min(SNIFF)];
        if head.contains(&0) {
            return not_text(binary_kind(bytes));
        }
        let (bom, body) = match bytes {
            [0xEF, 0xBB, 0xBF, rest @ ..] => (true, rest),
            _ => (false, bytes),
        };
        let Ok(raw) = std::str::from_utf8(body) else {
            return not_text("not valid UTF-8");
        };
        let crlf = raw.contains("\r\n");
        let final_newline = raw.ends_with('\n');
        Ok(Self {
            text: raw.replace("\r\n", "\n"),
            bom,
            crlf,
            final_newline,
        })
    }

    /// The bytes for new text, spelled the way the file was.
    #[must_use]
    pub fn encode(&self, text: &str) -> Vec<u8> {
        let mut text = text.to_owned();
        if self.final_newline && !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        } else if !self.final_newline && text.ends_with('\n') && !self.text.ends_with('\n') {
            text.pop();
        }
        if self.crlf {
            text = text.replace('\n', "\r\n");
        }
        let mut bytes = Vec::with_capacity(text.len() + 3);
        if self.bom {
            bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        bytes.extend_from_slice(text.as_bytes());
        bytes
    }

    #[must_use]
    pub fn line_ending(&self) -> &'static str {
        if self.crlf { "CRLF" } else { "LF" }
    }

    #[must_use]
    pub fn encoding(&self) -> &'static str {
        if self.bom { "UTF-8 with BOM" } else { "UTF-8" }
    }

    #[must_use]
    pub fn lines(&self) -> usize {
        if self.text.is_empty() {
            0
        } else {
            self.text.lines().count()
        }
    }
}

/// The hash a file is remembered by: the bytes as they are on disk, so a change made by any
/// other program is visible.
#[must_use]
pub fn hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Enough of a guess to stop the model trying again (D10).
fn binary_kind(bytes: &[u8]) -> &'static str {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => "a PNG image",
        [0xFF, 0xD8, 0xFF, ..] => "a JPEG image",
        [b'%', b'P', b'D', b'F', ..] => "a PDF document",
        [b'P', b'K', 0x03, 0x04, ..] => "a zip archive",
        [0x7F, b'E', b'L', b'F', ..] => "an executable",
        [b'G', b'I', b'F', b'8', ..] => "a GIF image",
        _ => "binary",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_endings_and_the_byte_order_mark_survive_a_round_trip() {
        let bytes = b"\xEF\xBB\xBFone\r\ntwo\r\n";
        let file = TextFile::decode("/x", bytes).unwrap();
        assert_eq!(file.text, "one\ntwo\n");
        assert!(file.bom && file.crlf && file.final_newline);
        assert_eq!(file.encode(&file.text.clone()), bytes);
        assert_eq!(file.encoding(), "UTF-8 with BOM");
        assert_eq!(file.line_ending(), "CRLF");
    }

    #[test]
    fn a_file_without_a_final_newline_does_not_grow_one() {
        let file = TextFile::decode("/x", b"one\ntwo").unwrap();
        assert!(!file.final_newline);
        assert_eq!(file.encode("one\nthree"), b"one\nthree");
    }

    #[test]
    fn binary_is_named_rather_than_decoded() {
        let err = TextFile::decode("/x.png", b"\x89PNG\r\n\x1a\n\0\0").unwrap_err();
        assert!(err.to_string().contains("a PNG image"), "{err}");
    }
}
