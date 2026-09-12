//! Migration 0012: the skills index and the memory store.
//!
//! The two reads that matter are the two the prompt makes — the core set and the long tail —
//! and each has a rule that is easy to get wrong: the core set must not contain a fact, and
//! the long tail must not repeat what the core set already froze into the prompt.

use gantry_core::{
    MemoryDto, MemoryId, MemoryKind, MemoryScopeKind, MemorySource, SkillDto, SkillSource,
    SkillVersionSource, now_ms,
};
use gantry_store::{
    Store,
    repos::{
        memories::{self, MemoryFilter},
        skills,
    },
};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("t.db")).unwrap();
    (dir, store)
}

fn skill(id: &str) -> SkillDto {
    let now = now_ms();
    SkillDto {
        id: id.to_owned(),
        source: SkillSource::User,
        path: Some(format!("/tmp/skills/{id}")),
        name: id.to_owned(),
        description: "Idiomatic Rust for this codebase.".into(),
        triggers: vec!["rust".into(), "borrow checker".into()],
        always_include: false,
        enabled: true,
        content_hash: "abc".into(),
        size: 120,
        version: 1,
        author: Some("olav".into()),
        license: None,
        references: vec!["errors.md".into()],
        installed_at: now,
        updated_at: now,
        last_used_at: None,
        use_count: 0,
        pinned_count: 0,
    }
}

fn memory(kind: MemoryKind, text: &str) -> MemoryDto {
    let now = now_ms();
    MemoryDto {
        id: MemoryId::new(),
        scope_kind: MemoryScopeKind::Global,
        scope_id: None,
        kind,
        text: text.to_owned(),
        always_include: false,
        source: MemorySource::User,
        origin_chat_id: None,
        origin_message_id: None,
        tags: Vec::new(),
        enabled: true,
        use_count: 0,
        last_used_at: None,
        created_at: now,
        updated_at: now,
        archived_at: None,
    }
}

#[test]
fn a_skill_round_trips_and_keeps_every_version() {
    let (_dir, store) = store();
    store
        .write_blocking(|c| {
            skills::upsert(c, &skill("rust-idioms"))?;
            skills::add_version(
                c,
                "rust-idioms",
                1,
                "---\nname: rust-idioms\n---\nv1",
                SkillVersionSource::UserEdit,
            )?;
            let mut edited = skill("rust-idioms");
            edited.version = 2;
            edited.description = "Now with tokio.".into();
            skills::upsert(c, &edited)?;
            skills::add_version(
                c,
                "rust-idioms",
                2,
                "---\nname: rust-idioms\n---\nv2",
                SkillVersionSource::UserEdit,
            )?;
            Ok(())
        })
        .unwrap();

    let got = store
        .read(|c| skills::get(c, "rust-idioms"))
        .unwrap()
        .expect("the skill is indexed");
    assert_eq!(got.version, 2);
    assert_eq!(got.description, "Now with tokio.");
    assert_eq!(got.triggers, vec!["rust", "borrow checker"]);
    assert_eq!(got.references, vec!["errors.md"]);

    // Every version is still readable, which is what makes Replace reversible (12 §A5).
    assert_eq!(
        store
            .read(|c| skills::last_version(c, "rust-idioms"))
            .unwrap(),
        2
    );
    let v1 = store
        .read(|c| skills::version_content(c, "rust-idioms", 1))
        .unwrap();
    assert_eq!(v1.as_deref(), Some("---\nname: rust-idioms\n---\nv1"));

    store
        .write_blocking(|c| skills::set_enabled(c, "rust-idioms", false))
        .unwrap();
    assert!(
        !store
            .read(skills::enabled)
            .unwrap()
            .iter()
            .any(|s| s.id == "rust-idioms")
    );
}

#[test]
fn the_core_set_is_standing_behaviour_and_nothing_else() {
    let (_dir, store) = store();
    let mut always_fact = memory(MemoryKind::Fact, "the API lives in services/api");
    always_fact.always_include = true;
    store
        .write_blocking(move |c| {
            memories::insert(c, &memory(MemoryKind::Instruction, "answer in Norwegian"))?;
            memories::insert(c, &memory(MemoryKind::Preference, "prefer pnpm"))?;
            memories::insert(
                c,
                &memory(MemoryKind::Fact, "the API lives in services/api"),
            )?;
            memories::insert(c, &memory(MemoryKind::Note, "the migration is half done"))?;
            memories::insert(c, &always_fact)?;
            Ok(())
        })
        .unwrap();

    let core = store.read(|c| memories::core_set(c, None)).unwrap();
    let texts: Vec<&str> = core.iter().map(|m| m.text.as_str()).collect();
    assert!(texts.contains(&"answer in Norwegian"));
    assert!(texts.contains(&"prefer pnpm"));
    // A fact is long tail — unless the user marked it Always, and then it is in both senses
    // standing, so it belongs here and nowhere else.
    assert_eq!(
        texts
            .iter()
            .filter(|t| **t == "the API lives in services/api")
            .count(),
        1,
        "only the Always copy of the fact is in the core set: {texts:?}"
    );
    assert!(!texts.contains(&"the migration is half done"));
}

#[test]
fn the_long_tail_is_found_by_words_and_never_repeats_the_core_set() {
    let (_dir, store) = store();
    let mut always = memory(MemoryKind::Fact, "the API lives in services/api");
    always.always_include = true;
    store
        .write_blocking(move |c| {
            memories::insert(c, &memory(MemoryKind::Fact, "the database is Postgres 16"))?;
            memories::insert(c, &memory(MemoryKind::Preference, "prefer pnpm"))?;
            memories::insert(c, &always)?;
            Ok(())
        })
        .unwrap();

    let hits = store
        .read(|c| memories::long_tail(c, None, &["database".to_owned(), "postgres".to_owned()], 8))
        .unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].text, "the database is Postgres 16");

    // A preference and an Always entry are already in the frozen prompt; asking about them
    // must not send them a second time.
    assert!(
        store
            .read(|c| memories::long_tail(c, None, &["pnpm".to_owned(), "api".to_owned()], 8))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn deleting_is_reversible_for_thirty_days() {
    let (_dir, store) = store();
    let m = memory(MemoryKind::Fact, "the API lives in services/api");
    let id = m.id;
    store
        .write_blocking(move |c| memories::insert(c, &m))
        .unwrap();
    store
        .write_blocking(move |c| memories::archive(c, id))
        .unwrap();

    assert!(
        store
            .read(|c| memories::list(c, MemoryFilter::default(), ""))
            .unwrap()
            .is_empty()
    );
    let deleted = store
        .read(|c| {
            memories::list(
                c,
                MemoryFilter {
                    archived: true,
                    ..MemoryFilter::default()
                },
                "",
            )
        })
        .unwrap();
    assert_eq!(deleted.len(), 1);
    assert!(
        store
            .read(|c| memories::long_tail(c, None, &["api".to_owned(), "services".to_owned()], 8))
            .unwrap()
            .is_empty(),
        "an archived entry never reaches a prompt"
    );

    store
        .write_blocking(move |c| memories::restore(c, id))
        .unwrap();
    assert_eq!(
        store
            .read(|c| memories::list(c, MemoryFilter::default(), ""))
            .unwrap()
            .len(),
        1
    );
}
