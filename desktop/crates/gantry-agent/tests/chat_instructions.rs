//! A chat's own standing instructions (docs/plan/10 §2, layer 6): where they sit among the other
//! two instruction layers, and what an open chat is told when they change.

use std::sync::{Arc, RwLock};

use gantry_agent::{
    ChatBook, ChatPatch, Projects, PromptContext, TurnManager,
    turn_manager::{ChatNotifier, ProviderSource},
};
use gantry_core::{
    ChatId, ModelRef, NewProject, ProjectId, ProjectPatch, ProviderId, Settings, Surface,
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

/// A world whose global custom instructions are already set, so that the three instruction
/// layers can be told apart by their text.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).unwrap());
    let chats = Arc::new(ChatBook::new(store.clone(), blobs.clone()));
    let mut settings = Settings::default();
    settings.chat.custom_instructions = "Never use emoji.".into();
    // Gantry ships with no default model (11 §1); these chats are created with `None`, so the
    // harness picks what the user would have.
    settings.chat.default_model = Some(ModelRef::new(ProviderId::openrouter(), "test/model"));
    let m = TurnManager::new(
        chats,
        Arc::new(NoProviders),
        Arc::new(gantry_connectors::ConnectorRegistry::new()),
        Arc::new(RwLock::new(settings)),
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
    fn project_answering_in_norwegian(&self) -> ProjectId {
        let id = self
            .projects
            .create(NewProject {
                name: "Gantry".to_owned(),
                description: String::new(),
                instructions: String::new(),
                workspace_path: None,
            })
            .unwrap()
            .id;
        self.projects
            .update(
                id,
                ProjectPatch {
                    instructions: Some("Answer in Norwegian.".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        id
    }

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
                        model: gantry_core::ModelRef::new(
                            gantry_core::ProviderId::openrouter(),
                            "test/model",
                        ),
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

    fn notes(&self, chat: ChatId) -> Vec<String> {
        self.store
            .read(move |c| repos::messages::list_for_chat(c, chat))
            .unwrap()
            .iter()
            .flat_map(|m| m.message.parts.clone())
            .filter_map(|p| match p {
                gantry_core::ContentPart::SystemNote { text } => Some(text),
                _ => None,
            })
            .collect()
    }
}

fn set(w: &World, chat: ChatId, text: &str) {
    w.m.update_chat(
        chat,
        ChatPatch {
            instructions: Some(text.to_owned()),
            ..Default::default()
        },
    )
    .unwrap();
}

#[tokio::test]
async fn a_chats_own_instructions_are_the_last_of_the_three_layers() {
    let w = world();
    let project = w.project_answering_in_norwegian();
    let chat =
        w.m.create_session(Surface::Chat, Vec::new(), None, false, Some(project))
            .unwrap();

    set(&w, chat.id, "Keep replies under three sentences.");

    let prompt = w.snapshot(chat.id);
    assert!(
        prompt.contains(
            "<instructions scope=\"chat\">\nKeep replies under three sentences.\n</instructions>"
        ),
        "{prompt}"
    );
    // `rfind`, not `find`: the core explains the layers by name before any of them is filled in.
    let at = |tag: &str| {
        prompt
            .rfind(tag)
            .unwrap_or_else(|| panic!("no {tag} in {prompt}"))
    };
    assert!(at("<instructions scope=\"global\">") < at("<instructions scope=\"project\">"));
    assert!(at("<instructions scope=\"project\">") < at("<instructions scope=\"chat\">"));
}

#[tokio::test]
async fn the_editor_reads_back_what_was_saved() {
    let w = world();
    let chat = w.m.create_chat(None).unwrap();
    set(&w, chat.id, "  Keep replies short.  ");
    let detail = w.m.chats().get(chat.id).unwrap().unwrap();
    assert_eq!(detail.instructions, "Keep replies short.");
}

#[tokio::test]
async fn a_chat_that_has_spoken_is_told_and_keeps_its_prompt() {
    let w = world();
    let untouched = w.m.create_chat(None).unwrap();
    let spoken = w.m.create_chat(None).unwrap();
    w.pretend_it_spoke(spoken.id);

    set(&w, untouched.id, "Keep replies short.");
    set(&w, spoken.id, "Keep replies short.");

    // The one with nothing in it is rebuilt around the new layer…
    assert!(
        w.snapshot(untouched.id)
            .contains("<instructions scope=\"chat\">\nKeep replies short."),
        "the untouched chat was not refrozen"
    );
    // …and the one that has spoken keeps the prompt it was answering and is told instead.
    assert!(!w.snapshot(spoken.id).contains("Keep replies short."));
    let notes = w.notes(spoken.id);
    assert!(
        notes.iter().any(|n| n.contains(
            "Updated this chat's instructions (replacing any earlier ones):\n<instructions scope=\"chat\">\nKeep replies short.\n</instructions>"
        )),
        "{notes:?}"
    );
}

#[tokio::test]
async fn clearing_them_says_so_rather_than_falling_silent() {
    let w = world();
    let chat = w.m.create_chat(None).unwrap();
    w.pretend_it_spoke(chat.id);
    set(&w, chat.id, "Keep replies short.");
    set(&w, chat.id, "   ");

    let notes = w.notes(chat.id);
    assert!(
        notes
            .iter()
            .any(|n| n.contains("removed this chat's own instructions")),
        "{notes:?}"
    );
}

#[tokio::test]
async fn saving_the_same_text_again_tells_nobody() {
    let w = world();
    let chat = w.m.create_chat(None).unwrap();
    w.pretend_it_spoke(chat.id);
    set(&w, chat.id, "Keep replies short.");
    set(&w, chat.id, "Keep replies short.\n");

    assert_eq!(w.notes(chat.id).len(), 1, "{:?}", w.notes(chat.id));
}
