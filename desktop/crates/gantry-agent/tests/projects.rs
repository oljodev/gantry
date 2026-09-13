//! What a project does to the chats in it (09 M11): the defaults they open with, the layers
//! they carry, and what an open one is told when the project changes underneath it.

use std::sync::{Arc, RwLock};

use gantry_agent::{
    ChatBook, Projects, PromptContext, TurnManager,
    memory::Memories,
    turn_manager::{ChatNotifier, ProviderSource},
};
use gantry_core::{
    ChatId, MemoryKind, MemoryScopeKind, MemorySource, Mode, NewProject, ProjectDefaults,
    ProjectGrant, ProjectId, ProjectPatch, ProviderId, RiskTier, Settings, SkillInput,
    SkillVersionSource, Surface,
};
use gantry_store::{BlobStore, Store, repos};

struct NoProviders;
impl ProviderSource for NoProviders {
    fn provider(&self, _id: &ProviderId) -> Option<Arc<dyn gantry_providers::Provider>> {
        None
    }
}

struct Quiet;
impl ChatNotifier for Quiet {
    fn chats_changed(&self, _ids: Vec<ChatId>) {}
    fn interactions_changed(&self, _chat_id: ChatId, _pending: u32) {}
}

struct World {
    _dir: tempfile::TempDir,
    m: Arc<TurnManager>,
    projects: Projects,
    store: Arc<Store>,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).unwrap());
    let chats = Arc::new(ChatBook::new(store.clone(), blobs.clone()));
    let m = TurnManager::new(
        chats,
        Arc::new(NoProviders),
        Arc::new(gantry_connectors::ConnectorRegistry::new()),
        Arc::new(RwLock::new(Settings::default())),
        PromptContext::default(),
        tokio::runtime::Handle::current(),
    );
    m.set_notifier(Arc::new(Quiet));
    World {
        _dir: dir,
        m,
        projects: Projects::new(store.clone(), blobs),
        store,
    }
}

impl World {
    fn project(&self, name: &str) -> ProjectId {
        self.projects
            .create(NewProject {
                name: name.to_owned(),
                description: String::new(),
                instructions: String::new(),
                workspace_path: None,
            })
            .unwrap()
            .id
    }

    /// A turn in the chat, so that `has_turns` is true: the difference between a chat that is
    /// rebuilt and one that is told is whether anything has been said in it.
    fn pretend_it_spoke(&self, chat: ChatId) {
        self.store
            .write_blocking(move |c| {
                repos::turns::insert(
                    c,
                    &repos::turns::TurnRecord {
                        id: gantry_core::TurnId::new(),
                        chat_id: chat,
                        seq: 1,
                        status: gantry_core::TurnStatus::Completed,
                        model: gantry_core::ModelRef::default_model(),
                        started_at: gantry_core::now_ms(),
                        ended_at: Some(gantry_core::now_ms()),
                        usage: None,
                        stop_reason: None,
                        error: None,
                        feedback: None,
                        tool_call_count: 0,
                    },
                )
            })
            .unwrap();
    }

    fn snapshot(&self, chat: ChatId) -> String {
        self.store
            .read(move |c| Ok(repos::chats::get(c, chat)?.unwrap().system_snapshot))
            .unwrap()
    }
}

#[tokio::test]
async fn a_chat_in_a_project_carries_its_name_instructions_knowledge_and_pinned_skill() {
    let w = world();
    let id = w.project("Gantry");
    w.projects
        .update(
            id,
            ProjectPatch {
                instructions: Some("Answer in Norwegian.".into()),
                ..Default::default()
            },
        )
        .unwrap();
    w.projects
        .add_file(
            id,
            gantry_core::AttachmentInput::Bytes {
                name: "spec.md".into(),
                mime: "text/markdown".into(),
                data_base64: base64("# The gantry\nIt lifts things."),
            },
        )
        .unwrap();
    let skills = gantry_agent::Skills::new(w.store.clone(), w._dir.path().join("skills"));
    skills
        .save(
            &SkillInput {
                name: "house-style".into(),
                description: "How this house writes, for anything written here.".into(),
                triggers: Vec::new(),
                always_include: false,
                author: None,
                license: None,
                body: "Short sentences.".into(),
                references: Vec::new(),
            },
            SkillVersionSource::UserEdit,
        )
        .unwrap();
    w.projects.pin_skill(id, "house-style", true).unwrap();
    // A memory in this project, and one in no project: both should reach a chat here.
    let memories = Memories::new(w.store.clone());
    memories
        .create(
            "prefer pnpm",
            MemoryKind::Preference,
            MemoryScopeKind::Project,
            Some(id),
            MemorySource::User,
            None,
        )
        .unwrap();
    memories
        .create(
            "call me Olav",
            MemoryKind::Preference,
            MemoryScopeKind::Global,
            None,
            MemorySource::User,
            None,
        )
        .unwrap();

    let chat =
        w.m.create_session(Surface::Chat, Vec::new(), None, false, Some(id))
            .unwrap();
    assert_eq!(chat.project_id, Some(id));
    let prompt = w.snapshot(chat.id);
    assert!(prompt.contains("project: Gantry"), "{prompt}");
    assert!(
        prompt.contains("<instructions scope=\"project\">\nAnswer in Norwegian."),
        "{prompt}"
    );
    assert!(prompt.contains("<file name=\"spec.md\">"), "{prompt}");
    assert!(prompt.contains("It lifts things."), "{prompt}");
    assert!(prompt.contains("<skill name=\"house-style\""), "{prompt}");
    assert!(prompt.contains("Short sentences."), "{prompt}");
    assert!(prompt.contains("prefer pnpm"), "{prompt}");
    assert!(prompt.contains("call me Olav"), "{prompt}");

    // Order is the layering of 10 §2: facts before rules, and the most specific rule last.
    // `rfind`, because the core prompt names these tags when it explains them, and the blocks
    // themselves are below everything the core says about them.
    let at = |needle: &str| prompt.rfind(needle).unwrap_or_else(|| panic!("{needle}"));
    assert!(at("<gantry_context>") < at("<memory>"));
    assert!(at("<memory>") < at("<project_knowledge>"));
    assert!(at("<project_knowledge>") < at("<instructions scope=\"project\">"));
    assert!(at("<instructions scope=\"project\">") < at("<skill name="));

    // A chat outside the project gets none of it, and keeps the global memory.
    let loose = w.m.create_chat(None).unwrap();
    let prompt = w.snapshot(loose.id);
    assert!(!prompt.contains("project:"), "{prompt}");
    assert!(!prompt.contains("Answer in Norwegian."), "{prompt}");
    assert!(!prompt.contains("prefer pnpm"), "{prompt}");
    assert!(prompt.contains("call me Olav"), "{prompt}");
}

#[tokio::test]
async fn the_defaults_a_project_sets_are_the_ones_a_new_chat_opens_with() {
    let w = world();
    let id = w.project("Gantry");
    w.projects
        .update(
            id,
            ProjectPatch {
                workspace_path: Some(Some("/home/olav/dev/gantry".into())),
                defaults: Some(ProjectDefaults {
                    mode: Some(Mode::Plan),
                    guard: None,
                    connectors: None,
                    grants: Some(vec![ProjectGrant {
                        instance_name: "filesystem".into(),
                        tool_name: None,
                        tier_ceiling: Some(RiskTier::Read),
                    }]),
                }),
                ..Default::default()
            },
        )
        .unwrap();

    let chat =
        w.m.create_session(Surface::Chat, Vec::new(), None, false, Some(id))
            .unwrap();
    let detail = w.m.chats().get(chat.id).unwrap().unwrap();
    assert_eq!(detail.mode, Mode::Plan);
    // The project says nothing about the guard, so the settings do — not "off".
    assert_eq!(detail.guard, Settings::default().chat.default_guard);
    assert_eq!(detail.roots, ["/home/olav/dev/gantry"]);
    let grants = w
        .store
        .read(move |c| repos::grants::active(c, chat.id))
        .unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].source, gantry_core::GrantSource::ProjectDefault);
    assert_eq!(grants[0].tier_ceiling, Some(RiskTier::Read));

    // A folder the user picked for this session wins over the project's.
    let code =
        w.m.create_session(
            Surface::Code,
            vec!["/home/olav/dev/other".into()],
            None,
            false,
            Some(id),
        )
        .unwrap();
    assert_eq!(code.roots, ["/home/olav/dev/other"]);

    // Incognito takes the project's context but none of its standing permissions.
    let private =
        w.m.create_session(Surface::Chat, Vec::new(), None, true, Some(id))
            .unwrap();
    assert!(
        w.store
            .read(move |c| repos::grants::active(c, private.id))
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_chat_that_has_spoken_is_told_what_changed_and_a_new_one_is_rebuilt() {
    let w = world();
    let id = w.project("Gantry");
    let untouched =
        w.m.create_session(Surface::Chat, Vec::new(), None, false, Some(id))
            .unwrap();
    let spoken =
        w.m.create_session(Surface::Chat, Vec::new(), None, false, Some(id))
            .unwrap();
    w.pretend_it_spoke(spoken.id);

    w.projects
        .update(
            id,
            ProjectPatch {
                instructions: Some("Answer in Norwegian.".into()),
                ..Default::default()
            },
        )
        .unwrap();
    w.m.project_changed(
        id,
        "Updated project instructions:\n<instructions scope=\"project\">\nAnswer in Norwegian.\n</instructions>".into(),
    )
    .unwrap();

    // The one with nothing in it is rebuilt from the new project…
    assert!(
        w.snapshot(untouched.id).contains("Answer in Norwegian."),
        "the untouched chat was not refrozen"
    );
    // …and the one that has spoken keeps its prompt and is told instead, because rewriting a
    // prompt under a conversation rewrites what the model was answering.
    assert!(!w.snapshot(spoken.id).contains("Answer in Norwegian."));
    let detail = w.m.chats().get(spoken.id).unwrap().unwrap();
    let notes = w
        .store
        .read(move |c| repos::messages::list_for_chat(c, spoken.id))
        .unwrap();
    assert!(
        notes
            .iter()
            .any(|m| {
                m.message.parts.iter().any(|p| matches!(
            p,
            gantry_core::ContentPart::SystemNote { text } if text.contains("Answer in Norwegian.")
        ))
            }),
        "the chat that has spoken was not told; it has {} turns",
        detail.turns.len()
    );
}

#[tokio::test]
async fn moving_a_chat_into_a_project_changes_what_applies_from_here_on() {
    let w = world();
    let id = w.project("Gantry");
    w.projects
        .update(
            id,
            ProjectPatch {
                instructions: Some("Answer in Norwegian.".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let chat = w.m.create_chat(None).unwrap();
    w.m.set_chat_project(chat.id, Some(id)).unwrap();

    let detail = w.m.chats().get(chat.id).unwrap().unwrap();
    assert_eq!(detail.project_id, Some(id));
    // Nothing had been said in it, so it is simply rebuilt as a chat of this project.
    let prompt = w.snapshot(chat.id);
    assert!(prompt.contains("Answer in Norwegian."), "{prompt}");
    assert!(prompt.contains("project: Gantry"), "{prompt}");

    w.m.set_chat_project(chat.id, None).unwrap();
    assert!(!w.snapshot(chat.id).contains("Answer in Norwegian."));
}

#[tokio::test]
async fn knowledge_that_would_not_fit_is_cut_and_says_so() {
    let w = world();
    let id = w.project("Gantry");
    let long = "x".repeat(gantry_core::PROJECT_KNOWLEDGE_MAX_CHARS);
    for (name, text) in [("big.md", long.as_str()), ("small.md", "the short one")] {
        w.projects
            .add_file(
                id,
                gantry_core::AttachmentInput::Bytes {
                    name: name.into(),
                    mime: "text/markdown".into(),
                    data_base64: base64(text),
                },
            )
            .unwrap();
    }
    let chat =
        w.m.create_session(Surface::Chat, Vec::new(), None, false, Some(id))
            .unwrap();
    let prompt = w.snapshot(chat.id);
    // The small file is whole — the budget is shared, not spent first-come, so one long file
    // cannot push the others out of the prompt entirely.
    assert!(
        prompt.contains("<file name=\"small.md\">\nthe short one"),
        "{prompt}"
    );
    assert!(
        prompt.contains("<file name=\"big.md\" included=\"the first "),
        "{prompt}"
    );
    assert!(
        prompt.len() < gantry_core::PROJECT_KNOWLEDGE_MAX_CHARS + 8000,
        "{}",
        prompt.len()
    );
}

#[tokio::test]
async fn a_file_with_no_text_in_it_is_refused_as_knowledge() {
    let w = world();
    let id = w.project("Gantry");
    let err = w
        .projects
        .add_file(
            id,
            gantry_core::AttachmentInput::Bytes {
                name: "logo.png".into(),
                mime: "image/png".into(),
                data_base64: base64("not really a png"),
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("no text in it"), "{err}");
}

fn base64(text: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(text)
}
