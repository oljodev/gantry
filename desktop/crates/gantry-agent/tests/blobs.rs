//! What happens to a blob's bytes when the thing holding them goes (06 §3): a knowledge file
//! removed from a project, a project deleted, a chat deleted, a turn retried.

use std::sync::Arc;

use gantry_agent::{
    ChatBook, Projects,
    chats::{NewChat, TurnContextOptions},
};
use gantry_core::{
    AttachmentInput, Message, Mode, ModelRef, NewProject, ProviderId, ReasoningEffort, Surface,
};
use gantry_store::{BlobStore, Store};

struct World {
    _dir: tempfile::TempDir,
    book: ChatBook,
    projects: Projects,
    blobs: Arc<BlobStore>,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).unwrap());
    World {
        _dir: dir,
        book: ChatBook::new(store.clone(), blobs.clone()),
        projects: Projects::new(store, blobs.clone()),
        blobs,
    }
}

impl World {
    fn has(&self, hash: &str) -> bool {
        self.blobs.get(hash).is_ok()
    }

    /// Moves a blob's file back an hour. Bytes are collected only once they have sat untouched
    /// for `sweep::GRACE`, which is what stops a sweep from deleting a file whose reference is
    /// milliseconds away from being written; everything below is about a file from a while ago.
    fn age(&self, hash: &str) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(self.blobs.path_for(hash))
            .unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
    }

    /// A file on disk to hand to `add_file`, which reads it the way the composer does.
    fn file(&self, name: &str, text: &str) -> AttachmentInput {
        let path = self._dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        AttachmentInput::Path {
            path: path.to_string_lossy().into_owned(),
        }
    }
}

fn chat(book: &ChatBook) -> gantry_core::ChatId {
    book.create(NewChat {
        surface: Surface::Chat,
        roots: Vec::new(),
        model: ModelRef::new(ProviderId::openrouter(), "test/model"),
        mode: Mode::Manual,
        guard: true,
        effort: ReasoningEffort::Off,
        system_snapshot: String::new(),
        system_snapshot_version: 1,
        connectors: Vec::new(),
        incognito: false,
        project: None,
        grants: Vec::new(),
        parent: None,
    })
    .unwrap()
    .id
}

fn project(name: &str) -> NewProject {
    NewProject {
        name: name.into(),
        description: String::new(),
        instructions: String::new(),
        workspace_path: None,
    }
}

#[test]
fn a_knowledge_file_takes_its_bytes_with_it() {
    let w = world();
    let project = w.projects.create(project("Ships")).unwrap();
    let added = w
        .projects
        .add_file(project.id, w.file("notes.md", "keel laying"))
        .unwrap();
    assert!(w.has(&added.blob_hash));

    w.age(&added.blob_hash);
    w.projects.remove_file(project.id, added.id).unwrap();
    assert!(
        !w.has(&added.blob_hash),
        "removing the file removed its bytes"
    );
}

#[test]
fn deleting_a_project_takes_its_knowledge_with_it() {
    let w = world();
    let project = w.projects.create(project("Ships")).unwrap();
    let added = w
        .projects
        .add_file(project.id, w.file("notes.md", "keel laying"))
        .unwrap();
    w.age(&added.blob_hash);
    w.projects.delete(project.id).unwrap();
    assert!(!w.has(&added.blob_hash));
}

#[test]
fn a_file_two_projects_know_survives_the_first_of_them() {
    let w = world();
    let mut ids = Vec::new();
    for name in ["Ships", "Cranes"] {
        let p = w.projects.create(project(name)).unwrap();
        ids.push((
            p.id,
            w.projects
                .add_file(p.id, w.file(&format!("{name}.md"), "the same words"))
                .unwrap(),
        ));
    }
    assert_eq!(ids[0].1.blob_hash, ids[1].1.blob_hash, "one file, two rows");

    w.age(&ids[0].1.blob_hash);
    w.projects.delete(ids[0].0).unwrap();
    assert!(w.has(&ids[1].1.blob_hash), "the other project still has it");
    w.projects.delete(ids[1].0).unwrap();
    assert!(!w.has(&ids[1].1.blob_hash));
}

#[test]
fn deleting_a_chat_takes_its_attachment_and_its_text_with_it() {
    let w = world();
    let id = chat(&w.book);
    // A PDF is stored twice on purpose: the file the user attached, and the text that is what
    // the prompt can carry. The text was the blob nothing counted.
    use base64::Engine;
    let ingested = gantry_agent::attachments::ingest(
        &w.blobs,
        vec![AttachmentInput::Bytes {
            name: "note.pdf".into(),
            mime: "application/pdf".into(),
            data_base64: base64::engine::general_purpose::STANDARD
                .encode(gantry_documents::sample::pdf(&["A note about gantries."])),
        }],
    )
    .unwrap();
    let mut user = Message::user_text("Read this");
    user.parts.push(ingested[0].part.clone());
    let record = ingested[0].record.clone();
    let document = record.blob_hash.clone();
    let gantry_core::ContentPart::Document {
        source: gantry_core::MediaSource::Blob { hash: text },
        ..
    } = ingested[0].part.clone()
    else {
        panic!("a document part")
    };
    assert_ne!(document, text, "two blobs, not one");
    w.book
        .begin_turn(id, user, vec![record], TurnContextOptions::default())
        .unwrap();
    assert!(w.has(&document) && w.has(&text));

    w.age(&document);
    w.age(&text);
    assert!(w.book.delete(id).unwrap());
    assert!(!w.has(&document), "the file the user attached");
    assert!(!w.has(&text), "and the text taken out of it");
}

#[test]
fn a_retry_never_lets_go_of_the_attachment_it_is_about_to_send_again() {
    let w = world();
    let id = chat(&w.book);
    let ingested = gantry_agent::attachments::ingest(
        &w.blobs,
        vec![AttachmentInput::Bytes {
            name: "main.rs".into(),
            mime: "text/x-rust".into(),
            data_base64: "Zm4gbWFpbigpIHt9".into(),
        }],
    )
    .unwrap();
    let mut user = Message::user_text("Review this");
    user.parts.push(ingested[0].part.clone());
    let record = ingested[0].record.clone();
    let hash = record.blob_hash.clone();
    let first = w
        .book
        .begin_turn(id, user, vec![record], TurnContextOptions::default())
        .unwrap();
    w.book.finish_turn(
        id,
        first.turn_id,
        gantry_agent::chats::TurnOutcome {
            status: gantry_core::TurnStatus::Failed,
            usage: None,
            stop_reason: None,
            error: Some("try again".into()),
            tool_call_count: 0,
        },
    );

    let (user, attachments) = w.book.last_turn_to_retry(id, first.turn_id).unwrap();
    assert_eq!(attachments.len(), 1);
    // The turn being retried is only taken away by the write that starts its replacement, so
    // there is no moment in which nothing references the file the retry is about to re-send.
    assert!(w.has(&hash));
    w.book
        .begin_turn(
            id,
            user,
            attachments,
            TurnContextOptions {
                replacing: Some(first.turn_id),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(w.has(&hash));
    assert_eq!(w.book.get(id).unwrap().unwrap().turns.len(), 1);
}
