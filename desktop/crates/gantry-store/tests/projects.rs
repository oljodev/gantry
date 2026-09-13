//! Migration 0014: a project, its knowledge files, and what happens to everything hanging off
//! one when it is deleted.

use gantry_core::{
    ChatId, Mode, ModelRef, ProjectDefaults, ProjectFileDto, ProjectFileId, ProjectGrant,
    ProjectId, ReasoningEffort, RiskTier, now_ms,
};
use gantry_store::{
    Store,
    repos::{artifacts, chats, memories, projects, skills},
};

fn open() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("t.db")).unwrap();
    (dir, store)
}

fn project(name: &str) -> projects::ProjectRecord {
    let now = now_ms();
    projects::ProjectRecord {
        id: ProjectId::new(),
        name: name.to_owned(),
        description: String::new(),
        instructions: String::new(),
        workspace_path: None,
        defaults: ProjectDefaults::default(),
        pinned: false,
        sort_order: 0,
        created_at: now,
        updated_at: now,
        archived_at: None,
    }
}

fn chat(project: Option<ProjectId>) -> chats::ChatRecord {
    let now = now_ms();
    chats::ChatRecord {
        surface: gantry_core::Surface::Chat,
        id: ChatId::new(),
        project_id: project,
        title: "a chat".into(),
        title_source: "auto".into(),
        pinned: false,
        mode: Mode::AutoEdit,
        guard: true,
        model: ModelRef::default_model(),
        effort: ReasoningEffort::Medium,
        web_search: false,
        instructions: String::new(),
        system_snapshot: "sys".into(),
        system_snapshot_version: 1,
        created_at: now,
        updated_at: now,
        last_message_at: now,
        archived_at: None,
        incognito: false,
    }
}

fn skill() -> gantry_core::SkillDto {
    gantry_core::SkillDto {
        id: "commit-messages".into(),
        source: gantry_core::SkillSource::Bundled,
        path: None,
        name: "commit-messages".into(),
        description: "How to write one.".into(),
        triggers: vec!["commit".into()],
        always_include: false,
        enabled: true,
        content_hash: "h".into(),
        size: 10,
        version: 1,
        author: None,
        license: None,
        references: Vec::new(),
        installed_at: now_ms(),
        updated_at: now_ms(),
        last_used_at: None,
        use_count: 0,
        pinned_count: 0,
    }
}

fn file(project_id: ProjectId, name: &str) -> ProjectFileDto {
    ProjectFileDto {
        id: ProjectFileId::new(),
        project_id,
        name: name.to_owned(),
        mime: "text/markdown".into(),
        size: 12,
        blob_hash: "hash".into(),
        text_chars: 0,
        created_at: now_ms(),
    }
}

#[test]
fn the_defaults_a_project_does_not_set_stay_unset() {
    let (_dir, store) = open();
    let mut p = project("Gantry");
    p.instructions = "Write Norwegian.".into();
    p.defaults = ProjectDefaults {
        mode: Some(Mode::Plan),
        guard: None,
        connectors: Some(vec!["filesystem".into()]),
        grants: Some(vec![ProjectGrant {
            instance_name: "filesystem".into(),
            tool_name: None,
            tier_ceiling: Some(RiskTier::Read),
        }]),
    };
    let id = p.id;
    store
        .write_blocking(move |c| projects::insert(c, &p))
        .unwrap();

    let back = store.read(move |c| projects::get(c, id)).unwrap().unwrap();
    assert_eq!(back.instructions, "Write Norwegian.");
    assert_eq!(back.defaults.mode, Some(Mode::Plan));
    // Not "the guard is off": the project does not say, so the settings do.
    assert_eq!(back.defaults.guard, None);
    assert_eq!(back.defaults.connectors.unwrap(), vec!["filesystem"]);
    assert_eq!(
        back.defaults.grants.unwrap()[0].tier_ceiling,
        Some(RiskTier::Read)
    );
}

#[test]
fn the_list_counts_the_chats_and_files_of_each_project() {
    let (_dir, store) = open();
    let a = project("A");
    let b = project("B");
    let (id_a, id_b) = (a.id, b.id);
    store
        .write_blocking(move |c| {
            projects::insert(c, &a)?;
            projects::insert(c, &b)?;
            chats::insert(c, &chat(Some(id_a)))?;
            chats::insert(c, &chat(Some(id_a)))?;
            let mut hidden = chat(Some(id_a));
            hidden.incognito = true;
            chats::insert(c, &hidden)?;
            chats::insert(c, &chat(None))?;
            projects::add_file(c, &file(id_a, "spec.md"), Some("hello"))
        })
        .unwrap();

    let list = store.read(projects::list).unwrap();
    let a = list.iter().find(|p| p.id == id_a).unwrap();
    let b = list.iter().find(|p| p.id == id_b).unwrap();
    // Two chats, not three: an incognito chat is in no list, and a project page is a list.
    assert_eq!((a.chat_count, a.file_count), (2, 1));
    assert_eq!((b.chat_count, b.file_count), (0, 0));

    let files = store.read(move |c| projects::files(c, id_a)).unwrap();
    assert_eq!(files[0].name, "spec.md");
    assert_eq!(files[0].text_chars, 5);
    let knowledge = store.read(move |c| projects::knowledge(c, id_a)).unwrap();
    assert_eq!(knowledge, vec![("spec.md".to_owned(), "hello".to_owned())]);
}

#[test]
fn a_file_with_no_text_is_listed_but_never_reaches_a_prompt() {
    let (_dir, store) = open();
    let p = project("A");
    let id = p.id;
    store
        .write_blocking(move |c| {
            projects::insert(c, &p)?;
            projects::add_file(c, &file(id, "scan.pdf"), None)
        })
        .unwrap();
    assert_eq!(
        store.read(move |c| projects::files(c, id)).unwrap().len(),
        1
    );
    assert!(
        store
            .read(move |c| projects::knowledge(c, id))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn deleting_a_project_releases_its_chats_rather_than_taking_them_with_it() {
    let (_dir, store) = open();
    let p = project("A");
    let id = p.id;
    let kept = chat(Some(id));
    let chat_id = kept.id;
    let memory = gantry_core::MemoryDto {
        id: gantry_core::MemoryId::new(),
        scope_kind: gantry_core::MemoryScopeKind::Project,
        scope_id: Some(id),
        kind: gantry_core::MemoryKind::Preference,
        text: "prefer pnpm".into(),
        always_include: false,
        source: gantry_core::MemorySource::User,
        origin_chat_id: None,
        origin_message_id: None,
        tags: Vec::new(),
        enabled: true,
        use_count: 0,
        last_used_at: None,
        created_at: now_ms(),
        updated_at: now_ms(),
        archived_at: None,
    };
    let artifact = store
        .write_blocking(move |c| {
            projects::insert(c, &p)?;
            chats::insert(c, &kept)?;
            memories::insert(c, &memory)?;
            projects::add_file(c, &file(id, "spec.md"), Some("hello"))?;
            artifacts::create(
                c,
                chat_id,
                Some(id.to_string()),
                "document",
                "Spec",
                None,
                None,
                None,
                &artifacts::NewVersion {
                    content_blob_hash: "hash".into(),
                    source: gantry_core::VersionSource::ModelCreate,
                    size: 5,
                    tool_call_id: None,
                    message_id: None,
                    note: None,
                    text: "hello".into(),
                },
            )
        })
        .unwrap();

    store
        .write_blocking(move |c| projects::delete(c, id))
        .unwrap();

    // The conversation survives, out of the project; so does the artifact, which belongs to it.
    let chat = store
        .read(move |c| chats::get(c, chat_id))
        .unwrap()
        .unwrap();
    assert_eq!(chat.project_id, None);
    let back = store
        .read(move |c| artifacts::get(c, artifact.id))
        .unwrap()
        .unwrap();
    assert_eq!(back.project_id, None);
    // The project's own rows go: its files with it, and a memory that was only ever offered to
    // its chats is archived rather than left applying to nobody.
    assert!(
        store
            .read(move |c| projects::files(c, id))
            .unwrap()
            .is_empty()
    );
    let live = store
        .read(|c| memories::core_set(c, None))
        .unwrap()
        .into_iter()
        .filter(|m| m.text == "prefer pnpm")
        .count();
    assert_eq!(live, 0);
    assert!(store.read(move |c| projects::get(c, id)).unwrap().is_none());
}

#[test]
fn moving_a_chat_into_a_project_moves_what_it_made() {
    let (_dir, store) = open();
    let p = project("A");
    let id = p.id;
    let loose = chat(None);
    let chat_id = loose.id;
    let artifact = store
        .write_blocking(move |c| {
            projects::insert(c, &p)?;
            chats::insert(c, &loose)?;
            artifacts::create(
                c,
                chat_id,
                None,
                "document",
                "Notes",
                None,
                None,
                None,
                &artifacts::NewVersion {
                    content_blob_hash: "hash".into(),
                    source: gantry_core::VersionSource::ModelCreate,
                    size: 5,
                    tool_call_id: None,
                    message_id: None,
                    note: None,
                    text: "hello".into(),
                },
            )
        })
        .unwrap();

    store
        .write_blocking(move |c| projects::set_chat_project(c, chat_id, Some(id)))
        .unwrap();
    let back = store
        .read(move |c| artifacts::get(c, artifact.id))
        .unwrap()
        .unwrap();
    // 13 §9: the chat owns the artifact, the project sees it. Moving the chat moves the view.
    assert_eq!(back.project_id, Some(id));
    assert_eq!(
        store.read(move |c| projects::chat_ids(c, id)).unwrap(),
        vec![chat_id]
    );
    assert_eq!(
        store
            .read(move |c| artifacts::list_for_project(c, &id.to_string()))
            .unwrap()
            .len(),
        1
    );

    store
        .write_blocking(move |c| projects::set_chat_project(c, chat_id, None))
        .unwrap();
    assert!(
        store
            .read(move |c| projects::chat_ids(c, id))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_pinned_skill_goes_with_the_project_it_was_pinned_to() {
    let (_dir, store) = open();
    let p = project("A");
    let id = p.id;
    store
        .write_blocking(move |c| {
            projects::insert(c, &p)?;
            skills::upsert(c, &skill())?;
            skills::pin_to_project(c, id, "commit-messages")
        })
        .unwrap();
    assert_eq!(
        store
            .read(move |c| skills::pinned_for_project(c, id))
            .unwrap(),
        vec!["commit-messages"]
    );
    // The foreign key migration 0014 added: deleting the project takes the pin with it, and the
    // skill itself is untouched.
    store
        .write_blocking(move |c| projects::delete(c, id))
        .unwrap();
    assert!(
        store
            .read(move |c| skills::pinned_for_project(c, id))
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .read(|c| skills::get(c, "commit-messages"))
            .unwrap()
            .is_some()
    );
}
