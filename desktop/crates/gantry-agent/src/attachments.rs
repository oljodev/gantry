//! Turning attachment inputs into blobs and message parts: text, images, and the documents
//! `gantry-documents` can turn into text.
//!
//! A document is stored twice, on purpose. The file itself is a blob, because the attachment is
//! that file and a later version of Gantry may hand it to a provider that reads PDFs natively;
//! its text is a second blob, because the text is what the message carries into the prompt, and
//! a provider handed a PDF's raw bytes as text receives line noise.

use std::path::Path;

use gantry_core::{
    AttachmentInput, ContentPart, GantryError, MAX_DOCUMENT_BYTES, MAX_IMAGE_BYTES, MAX_TEXT_BYTES,
    MediaSource,
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
            if p.is_dir() {
                return Err(GantryError::invalid(format!(
                    "{name} is a folder; attach files one by one (adding a folder to the workspace arrives with M6)"
                )));
            }
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
    if let Some(kind) = gantry_documents::kind_of(&name, &bytes) {
        if size > MAX_DOCUMENT_BYTES {
            return Err(GantryError::invalid(format!(
                "{name} is {} MB; documents are limited to {} MB",
                size / (1024 * 1024),
                MAX_DOCUMENT_BYTES / (1024 * 1024)
            )));
        }
        // Every refusal the extractor can give is already a sentence for a person: a scan with no
        // text in it, a password nobody has, a file that is damaged.
        let extracted = gantry_documents::extract(kind, &name, &bytes)
            .map_err(|err| GantryError::invalid(err.to_string()))?;
        if extracted.text.len() > MAX_TEXT_BYTES {
            return Err(GantryError::invalid(format!(
                "{name} is {} pages, which come to {} KB of text — more than the {} KB a message \
                 can carry. Add the folder it is in to the chat instead, so it can be read a \
                 piece at a time.",
                extracted.pages,
                extracted.text.len() / 1024,
                MAX_TEXT_BYTES / 1024
            )));
        }
        let hash = blobs.put(&bytes)?;
        let text = blobs.put(extracted.text.as_bytes())?;
        return Ok(Ingested {
            // The part points at the text, which is what a prompt can hold; the record points at
            // the document, which is what the user attached.
            part: ContentPart::Document {
                source: MediaSource::Blob { hash: text },
                mime: "text/plain".to_owned(),
                name: name.clone(),
            },
            record: NewAttachment {
                name,
                mime: kind.mime().to_owned(),
                size: size as i64,
                blob_hash: hash,
                extracted_text: Some(extracted.text),
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
            "{name} is not text, an image, or a document Gantry can read"
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

/// One knowledge file of a project (09 M11): the same reading, classifying, size-checking and
/// extracting as an attachment, landing as a row rather than as a message part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Knowledge {
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub blob_hash: String,
    /// What goes into the prompt of every chat in the project.
    pub text: String,
}

/// Reads one file into a project's knowledge.
///
/// An image is refused here although it is a perfectly good attachment: knowledge is frozen into
/// the prompt of every chat in the project, and a picture cannot be. Attaching it to a message,
/// where the model really does look at it, still works.
pub fn knowledge(blobs: &BlobStore, input: AttachmentInput) -> Result<Knowledge, GantryError> {
    let ingested = ingest_one(blobs, input)?;
    let record = ingested.record;
    let Some(text) = record.extracted_text else {
        return Err(GantryError::invalid(format!(
            "{} has no text in it, so it cannot be project knowledge. Attach it to a message \
             instead, where the model can look at it.",
            record.name
        )));
    };
    Ok(Knowledge {
        name: record.name,
        mime: record.mime,
        size: record.size,
        blob_hash: record.blob_hash,
        text,
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
    fn a_pdf_is_ingested_as_its_text_and_keeps_the_file_beside_it() {
        let (_dir, b) = blobs();
        let pdf = gantry_documents::sample::pdf(&["A note about gantries."]);
        use base64::Engine;
        let out = ingest(
            &b,
            vec![AttachmentInput::Bytes {
                name: "note.pdf".into(),
                mime: "application/pdf".into(),
                data_base64: base64::engine::general_purpose::STANDARD.encode(&pdf),
            }],
        )
        .unwrap();
        let text = out[0].record.extracted_text.as_deref().unwrap();
        assert!(text.contains("A note about gantries."), "{text}");
        assert_eq!(out[0].record.mime, "application/pdf");
        assert_eq!(out[0].record.size, pdf.len() as i64);
        // The row keeps the document, so nothing about the attachment is lost; the part carries
        // the text, because a provider handed a PDF's bytes as text receives line noise.
        assert_eq!(b.get(&out[0].record.blob_hash).unwrap(), pdf);
        let ContentPart::Document {
            source: MediaSource::Blob { hash },
            ..
        } = &out[0].part
        else {
            panic!("a document part, got {:?}", out[0].part)
        };
        assert_eq!(b.get(hash).unwrap(), text.as_bytes());
    }

    #[test]
    fn a_scan_says_it_has_no_text_rather_than_arriving_empty() {
        let (_dir, b) = blobs();
        use base64::Engine;
        let err = ingest(
            &b,
            vec![AttachmentInput::Bytes {
                name: "scan.pdf".into(),
                mime: "application/pdf".into(),
                data_base64: base64::engine::general_purpose::STANDARD
                    .encode(gantry_documents::sample::pdf(&[""])),
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("character recognition"), "{err}");
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
        assert!(
            err.to_string()
                .contains("not text, an image, or a document")
        );
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
