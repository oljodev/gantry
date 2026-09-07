//! What the composer sends along with a message (docs/plan/01 §5, the `+` menu): a file the
//! user picked or dropped, by path, or bytes pasted into the webview.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttachmentInput {
    /// A file on disk, chosen in the file dialog or dropped on the window.
    Path { path: String },
    /// Bytes the webview already holds (a pasted image or file).
    Bytes {
        name: String,
        mime: String,
        data_base64: String,
    },
}

/// Limits the ingest applies (bytes).
pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 256 * 1024;
