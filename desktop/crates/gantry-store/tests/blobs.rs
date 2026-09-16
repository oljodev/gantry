//! The blob sweep (06 §3, §8): what it collects, what it leaves, and the one way it could go
//! wrong that no reading of the code would catch.

use std::{collections::HashSet, sync::Arc, time::Duration};

use gantry_core::{ChatId, MessageId};
use gantry_store::{
    BlobStore, Store,
    repos::{blobs, chats, messages},
    sweep,
};

const NOW: Duration = Duration::ZERO;

struct World {
    _dir: tempfile::TempDir,
    store: Arc<Store>,
    blobs: BlobStore,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("gantry.db")).unwrap());
    let blobs = BlobStore::open(dir.path().join("blobs")).unwrap();
    World {
        _dir: dir,
        store,
        blobs,
    }
}

impl World {
    fn chat(&self) -> ChatId {
        let id = ChatId::new();
        let record = chats::ChatRecord {
            id,
            surface: gantry_core::Surface::Chat,
            project_id: None,
            title: "a chat".into(),
            title_source: "auto".into(),
            pinned: false,
            mode: gantry_core::Mode::Manual,
            guard: true,
            model: gantry_core::ModelRef::default_model(),
            effort: gantry_core::ReasoningEffort::Off,
            web_search: false,
            instructions: String::new(),
            system_snapshot: String::new(),
            system_snapshot_version: 1,
            created_at: 1,
            updated_at: 1,
            last_message_at: 1,
            archived_at: None,
            incognito: false,
        };
        self.store
            .write_blocking(move |c| chats::insert(c, &record))
            .unwrap();
        id
    }

    /// Stores `bytes` and hangs them off a user message as a media part, the way an image
    /// attachment reaches the transcript.
    fn message_with(&self, chat_id: ChatId, bytes: &[u8]) -> (MessageId, String) {
        let hash = self.blobs.put(bytes).unwrap();
        let id = MessageId::new();
        let part = gantry_core::ContentPart::Image {
            source: gantry_core::MediaSource::Blob { hash: hash.clone() },
            mime: "image/png".into(),
        };
        let record = messages::MessageRecord {
            message: gantry_core::Message {
                id,
                role: gantry_core::Role::User,
                parts: vec![part],
                origin: None,
                created_at: 1,
            },
            chat_id,
            turn_id: None,
            seq: 1,
            stop_reason: None,
            usage: None,
        };
        self.store
            .write_blocking(move |c| messages::insert(c, &record))
            .unwrap();
        (id, hash)
    }

    fn sweep(&self) -> gantry_store::SweepReport {
        let blobs = self.blobs.clone();
        self.store
            .write_blocking(move |c| sweep::sweep(c, &blobs, NOW))
            .unwrap()
    }

    /// Moves a blob's file back an hour, past the grace period the sweep gives fresh bytes.
    fn age(&self, hash: &str) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(self.blobs.path_for(hash))
            .unwrap();
        file.set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
    }

    fn reachable(&self) -> HashSet<String> {
        self.store.read(blobs::reachable).unwrap()
    }
}

#[test]
fn a_blob_nothing_points_at_is_collected_and_one_in_a_message_is_not() {
    let w = world();
    let chat = w.chat();
    let (_id, kept) = w.message_with(chat, b"a picture");
    let orphan = w.blobs.put(b"nobody wants this").unwrap();

    assert_eq!(w.reachable(), HashSet::from([kept.clone()]));
    let report = w.sweep();
    assert_eq!(report.files, 1);
    assert_eq!(report.bytes, b"nobody wants this".len() as u64);
    assert!(w.blobs.get(&orphan).is_err(), "the orphan is gone");
    assert_eq!(w.blobs.get(&kept).unwrap(), b"a picture");
    assert!(w.sweep().is_empty(), "a second sweep finds nothing");
}

#[test]
fn deleting_the_chat_is_what_makes_its_blob_collectable() {
    let w = world();
    let chat = w.chat();
    let (_id, hash) = w.message_with(chat, b"a picture");
    assert!(w.sweep().is_empty());

    w.store
        .write_blocking(move |c| chats::delete(c, chat))
        .unwrap();
    assert_eq!(w.sweep().files, 1);
    assert!(w.blobs.get(&hash).is_err());
}

#[test]
fn two_chats_holding_the_same_bytes_keep_them_until_both_are_gone() {
    let w = world();
    let (first, second) = (w.chat(), w.chat());
    let (_a, one) = w.message_with(first, b"the same picture");
    let (_b, two) = w.message_with(second, b"the same picture");
    assert_eq!(one, two, "content-addressed: one file, two references");

    w.store
        .write_blocking(move |c| chats::delete(c, first))
        .unwrap();
    assert!(w.sweep().is_empty(), "the second chat still holds it");
    w.store
        .write_blocking(move |c| chats::delete(c, second))
        .unwrap();
    assert_eq!(w.sweep().files, 1);
}

#[test]
fn bytes_written_a_moment_ago_are_left_for_the_reference_that_is_coming() {
    let w = world();
    let hash = w.blobs.put(b"ingested, not yet sent").unwrap();
    let blobs = w.blobs.clone();
    let report = w
        .store
        .write_blocking(move |c| sweep::sweep(c, &blobs, Duration::from_secs(300)))
        .unwrap();
    assert!(report.is_empty(), "inside the grace period");
    assert_eq!(w.blobs.get(&hash).unwrap(), b"ingested, not yet sent");
    // With the grace period behind it, the same file goes: an attachment ingested for a message
    // that was never sent is exactly what the sweep is for.
    assert_eq!(w.sweep().files, 1);
}

#[test]
fn a_half_written_blob_from_a_crash_is_collected() {
    let w = world();
    let part = w.blobs.root().join("ab");
    std::fs::create_dir_all(&part).unwrap();
    std::fs::write(part.join(format!("{}.part", "ab".repeat(32))), b"half").unwrap();
    assert_eq!(w.sweep().files, 1);
}

#[test]
fn a_catalogue_row_for_a_blob_nobody_holds_goes_with_it() {
    let w = world();
    let hash = w.blobs.put(b"recorded and then abandoned").unwrap();
    let recorded = hash.clone();
    w.store
        .write_blocking(move |c| blobs::record(c, &recorded, 27, Some("text/plain")))
        .unwrap();
    let report = w.sweep();
    assert_eq!((report.files, report.rows), (1, 1));
    assert!(w.store.read(blobs::catalogued).unwrap().is_empty());
    assert!(!hash.is_empty());
}

/// The one mistake the sweep cannot survive is a table it has never heard of: its blobs would
/// look unreferenced and be deleted under it. So the schema is asked rather than trusted — every
/// column whose name says it holds a blob hash must be one the sweep reads.
#[test]
fn no_blob_column_is_missing_from_the_sweep() {
    let w = world();
    let found: Vec<(String, String)> = w
        .store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT m.name, p.name FROM sqlite_master m JOIN pragma_table_info (m.name) p
                 WHERE m.type = 'table' AND p.name LIKE '%blob_hash%' ORDER BY m.name, p.name",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap();
    let mut swept: Vec<(String, String)> = blobs::HASH_COLUMNS
        .iter()
        .map(|(t, c)| ((*t).to_owned(), (*c).to_owned()))
        .collect();
    swept.sort();
    let mut found = found;
    found.sort();
    assert_eq!(
        found, swept,
        "a blob-holding column the sweep does not read would have its files deleted"
    );
}

#[test]
fn a_sweep_is_weekly_rather_than_every_time_the_app_opens() {
    let w = world();
    let orphan = w.blobs.put(b"nobody wants this").unwrap();
    w.age(&orphan);
    let blobs = w.blobs.clone();
    let first = w
        .store
        .write_blocking(move |c| sweep::if_due(c, &blobs))
        .unwrap();
    assert_eq!(first.map(|r| r.files), Some(1), "never swept: sweep");
    assert!(w.blobs.get(&orphan).is_err());

    let blobs = w.blobs.clone();
    let second = w
        .store
        .write_blocking(move |c| sweep::if_due(c, &blobs))
        .unwrap();
    assert!(second.is_none(), "swept a moment ago: not again");

    // A week later it is due again, whatever there is to find.
    let week_ago = gantry_core::now_ms() - sweep::EVERY.as_millis() as i64 - 1;
    w.store
        .write_blocking(move |c| {
            gantry_store::repos::settings::set(c, sweep::SWEPT_AT_KEY, &week_ago.to_string())
        })
        .unwrap();
    let blobs = w.blobs.clone();
    assert!(
        w.store
            .write_blocking(move |c| sweep::if_due(c, &blobs))
            .unwrap()
            .is_some()
    );
}
