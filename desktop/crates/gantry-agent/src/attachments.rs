//! Turning attachment inputs into blobs and message parts. Text files and images only in M2;
//! PDFs and other documents arrive with their extractors later.

use std::path::Path;

use gantry_core::{
    AttachmentInput, ContentPart, GantryError, MAX_IMAGE_BYTES, MAX_TEXT_BYTES, MediaSource,
};
use gantry_store::BlobStore;

use crate::chats::NewAttachment;

/// One ingested attachment: the part that goes into the user message and the row to record.
#[derive(Debug, Clone, PartialEq)]
pub struct Ingested {
    pub part: ContentPart,
    pub record: NewAttachment,
}

/// Reads, classifies, size-checks and stores every input. Fails on the first bad one so the
/// user can fix it before anything is sent.
pub fn ingest(
    blobs: &BlobStore,
    inputs: Vec<AttachmentInput>,
) -> Result<Vec<Ingested>, GantryError> {
    inputs.into_iter().map(|i| ingest_one(blobs, i)).collect()
}

fn ingest_one(blobs: &BlobStore, input: AttachmentInput) -> Result<Ingested, GantryError> {
    let (name, mime_hint, bytes) = match input {
        AttachmentInput::Path { path } => {
            let p = Path::new(&path);
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            let bytes = std::fs::read(p)
                .map_err(|e| GantryError::invalid(format!("could not read {name}: {e}")))?;
            (name, None, bytes)
        }
        AttachmentInput::Bytes {
            name,
            mime,
            data_base64,
        } => {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data_base64.trim())
                .map_err(|e| GantryError::invalid(format!("{name}: bad base64 ({e})")))?;
            (name, Some(mime), bytes)
        }
    };
    let mime = mime_hint
        .filter(|m| !m.is_empty() && m != "application/octet-stream")
        .unwrap_or_else(|| mime_from_name(&name).to_owned());
    let size = bytes.len();
    if mime.starts_with("image/") {
        if !matches!(
            mime.as_str(),
            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
        ) {
            return Err(GantryError::invalid(format!(
                "{name}: only PNG, JPEG, GIF and WebP images are supported"
            )));
        }
        if size > MAX_IMAGE_BYTES {
            return Err(GantryError::invalid(format!(
                "{name} is {} MB; images are limited to {} MB",
                size / (1024 * 1024),
                MAX_IMAGE_BYTES / (1024 * 1024)
            )));
        }
        let hash = blobs.put(&bytes)?;
        return Ok(Ingested {
            part: ContentPart::Image {
                source: MediaSource::Blob { hash: hash.clone() },
                mime: mime.clone(),
            },
            record: NewAttachment {
                name,
                mime,
                size: size as i64,
                blob_hash: hash,
                extracted_text: None,
            },
        });
    }
    if size > MAX_TEXT_BYTES {
        return Err(GantryError::invalid(format!(
            "{name} is {} KB; text files are limited to {} KB",
            size / 1024,
            MAX_TEXT_BYTES / 1024
        )));
    }
    if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
        return Err(GantryError::invalid(format!(
            "{name} is not a text file or an image; other documents are not supported yet"
        )));
    }
    let mime = if mime.starts_with("text/") || is_text_mime(&mime) {
        mime
    } else {
        "text/plain".to_owned()
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let hash = blobs.put(&bytes)?;
    Ok(Ingested {
        part: ContentPart::Document {
            source: MediaSource::Blob { hash: hash.clone() },
            mime: mime.clone(),
            name: name.clone(),
        },
        record: NewAttachment {
            name,
            mime,
            size: size as i64,
            blob_hash: hash,
            extracted_text: Some(text),
        },
    })
}

fn is_text_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/json"
            | "application/xml"
            | "application/toml"
            | "application/yaml"
            | "application/x-yaml"
            | "application/javascript"
            | "application/typescript"
            | "application/x-sh"
            | "image/svg+xml"
    )
}

/// A small map from extension to mime; enough to tell images from text without a crate.
pub fn mime_from_name(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "md" | "markdown" => "text/markdown",
        "txt" | "log" | "" => "text/plain",
        "json" => "application/json",
        "toml" => "application/toml",
        "yaml" | "yml" => "application/yaml",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" | "cjs" => "application/javascript",
        "ts" | "tsx" | "jsx" => "application/typescript",
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "go" => "text/x-go",
        "java" | "kt" | "swift" | "c" | "h" | "cpp" | "hpp" | "cs" | "rb" | "php" | "sql"
        | "sh" | "fish" | "zsh" | "bash" | "csv" | "tsv" | "ini" | "cfg" | "conf" | "env"
        | "lock" | "diff" | "patch" => "text/plain",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blobs() -> (tempfile::TempDir, BlobStore) {
        let dir = tempfile::tempdir().unwrap();
        let b = BlobStore::open(dir.path().join("blobs")).unwrap();
        (dir, b)
    }

    #[test]
    fn text_files_become_documents_and_images_stay_binary() {
        let (dir, b) = blobs();
        let path = dir.path().join("notes.md");
        std::fs::write(&path, "# hi\n").unwrap();
        let out = ingest(
            &b,
            vec![
                AttachmentInput::Path {
                    path: path.to_string_lossy().into_owned(),
                },
                AttachmentInput::Bytes {
                    name: "shot.png".into(),
                    mime: "image/png".into(),
                    data_base64: "iVBORw0KGgo=".into(),
                },
            ],
        )
        .unwrap();
        assert!(
            matches!(&out[0].part, ContentPart::Document { name, mime, .. } if name == "notes.md" && mime == "text/markdown")
        );
        assert_eq!(out[0].record.extracted_text.as_deref(), Some("# hi\n"));
        assert!(matches!(&out[1].part, ContentPart::Image { mime, .. } if mime == "image/png"));
        assert_eq!(out[1].record.size, 8);
        assert_eq!(b.get(&out[1].record.blob_hash).unwrap().len(), 8);
    }

    #[test]
    fn binaries_and_oversized_files_are_refused() {
        let (_dir, b) = blobs();
        let err = ingest(
            &b,
            vec![AttachmentInput::Bytes {
                name: "a.bin".into(),
                mime: "application/octet-stream".into(),
                data_base64: "AAEC".into(),
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("not a text file"));
        let big = "a".repeat(MAX_TEXT_BYTES + 1);
        use base64::Engine;
        let err = ingest(
            &b,
            vec![AttachmentInput::Bytes {
                name: "big.txt".into(),
                mime: "text/plain".into(),
                data_base64: base64::engine::general_purpose::STANDARD.encode(big),
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("limited"));
        assert!(
            ingest(
                &b,
                vec![AttachmentInput::Path {
                    path: "/definitely/missing.txt".into()
                }]
            )
            .is_err()
        );
    }
}
