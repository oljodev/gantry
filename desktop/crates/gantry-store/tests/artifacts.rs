//! Migration 0004: artifacts and their versions.

use gantry_core::{ChatId, Mode, ModelRef, ReasoningEffort, VersionSource};
use gantry_store::{
    Store,
    repos::{
        artifacts::{self, NewVersion},
        chats::{self, ChatRecord},
    },
};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("t.db")).unwrap();
    (dir, store)
}

fn chat(store: &Store) -> ChatId {
    let id = ChatId::new();
    let now = gantry_core::now_ms();
    store
        .write_blocking(move |c| {
            chats::insert(
                c,
                &ChatRecord {
                    surface: gantry_core::Surface::Chat,
                    id,
                    project_id: None,
                    title: "T".into(),
                    title_source: "auto".into(),
                    pinned: false,
                    mode: Mode::AutoEdit,
                    guard: true,
                    model: ModelRef::default_model(),
                    effort: ReasoningEffort::Off,
                    web_search: false,
                    instructions: String::new(),
                    system_snapshot: String::new(),
                    system_snapshot_version: 1,
                    created_at: now,
                    updated_at: now,
                    last_message_at: now,
                    archived_at: None,
                    incognito: false,
                },
            )
        })
        .unwrap();
    id
}

fn version(source: VersionSource, text: &str) -> NewVersion {
    NewVersion {
        content_blob_hash: format!("{:0>64}", text.len()),
        size: text.len() as u64,
        source,
        tool_call_id: None,
        message_id: None,
        note: None,
        text: text.to_owned(),
    }
}

#[test]
fn versions_append_and_the_current_one_moves() {
    let (_d, store) = store();
    let chat_id = chat(&store);
    let a = store
        .write_blocking(move |c| {
            artifacts::create(
                c,
                chat_id,
                None,
                "markdown",
                "Plan",
                None,
                Some("A plan"),
                None,
                &version(VersionSource::ModelCreate, "# Plan\n"),
            )
        })
        .unwrap();
    assert_eq!(a.current_version, 1);
    assert_eq!(a.artifact_type, "markdown");
    let id = a.id;
    let v2 = store
        .write_blocking(move |c| {
            artifacts::add_version(
                c,
                id,
                Some("Plan v2"),
                None,
                &version(VersionSource::UserEdit, "# Plan\n\nMore.\n"),
            )
        })
        .unwrap();
    assert_eq!(v2, 2);
    let (row, versions, hash) = store
        .read(move |c| {
            Ok((
                artifacts::get(c, id)?.unwrap(),
                artifacts::versions(c, id)?,
                artifacts::content_hash(c, id, 2)?,
            ))
        })
        .unwrap();
    assert_eq!(row.title, "Plan v2");
    assert_eq!(row.summary.as_deref(), Some("A plan"), "summary kept");
    assert_eq!(row.current_version, 2);
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[1].source, VersionSource::UserEdit);
    assert_eq!(
        hash.unwrap(),
        version(VersionSource::UserEdit, "# Plan\n\nMore.\n").content_blob_hash
    );
    let listed = store
        .read(move |c| artifacts::list_for_chat(c, chat_id))
        .unwrap();
    assert_eq!(listed.len(), 1);
    let hashes = store
        .read(move |c| artifacts::hashes_for_chat(c, chat_id))
        .unwrap();
    assert_eq!(hashes.len(), 2);
}

#[test]
fn deleting_the_chat_takes_the_artifacts_with_it() {
    let (_d, store) = store();
    let chat_id = chat(&store);
    let a = store
        .write_blocking(move |c| {
            artifacts::create(
                c,
                chat_id,
                None,
                "code",
                "main.rs",
                Some("rust"),
                None,
                None,
                &version(VersionSource::ModelCreate, "fn main() {}"),
            )
        })
        .unwrap();
    store
        .write_blocking(move |c| chats::delete(c, chat_id))
        .unwrap();
    let id = a.id;
    let (row, versions) = store
        .read(move |c| Ok((artifacts::get(c, id)?, artifacts::versions(c, id)?)))
        .unwrap();
    assert!(row.is_none());
    assert!(versions.is_empty());
}
