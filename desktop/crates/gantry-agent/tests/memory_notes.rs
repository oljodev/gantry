//! What an open chat is told when a memory changes (docs/plan/12 §B6, 10 §4).
//!
//! The rule is the one every prompt layer follows — a chat that has not spoken is rebuilt, one
//! that has is told — with the addition memory brings: it is chosen in two tiers (§B4), so only
//! the tier that is *frozen* into a prompt has anything to announce. A `fact` looked up per
//! message needs no note, and would otherwise put one into every open conversation twice a turn
//! under auto-save.

use std::sync::{Arc, RwLock};

use gantry_agent::{
    ChatBook, Memories, Projects, PromptContext, TurnManager,
    turn_manager::{ChatNotifier, ProviderSource},
};
use gantry_core::{
    ChatId, MemoryDto, MemoryInput, MemoryKind, MemoryScopeKind, MemorySource, NewProject,
    ProjectId, ProviderId, Settings, Surface,
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
    memories: Arc<Memories>,
    projects: Projects,
    store: Arc<Store>,
}

/// The two halves wired the way `startup.rs` wires them: every write to the store announces
/// itself to the turn manager, which decides who hears about it.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).unwrap());
    let m = TurnManager::new(
        Arc::new(ChatBook::new(store.clone(), blobs.clone())),
        Arc::new(NoProviders),
        Arc::new(gantry_connectors::ConnectorRegistry::new()),
        Arc::new(RwLock::new(Settings::default())),
        PromptContext::default(),
        tokio::runtime::Handle::current(),
    );
    m.set_notifier(Arc::new(Quiet));
    let memories = Memories::new(store.clone());
    memories.set_on_change({
        let turns = Arc::downgrade(&m);
        Arc::new(move |entry: &MemoryDto, edit, except| {
            if let Some(turns) = turns.upgrade() {
                turns.memory_changed(entry, edit, except).unwrap();
            }
        })
    });
    World {
        _dir: dir,
        m,
        memories,
        projects: Projects::new(store.clone(), blobs),
        store,
    }
}

impl World {
    fn remember(&self, text: &str, kind: MemoryKind) -> MemoryDto {
        self.memories
            .create(
                text,
                kind,
                MemoryScopeKind::Global,
                None,
                MemorySource::User,
                None,
            )
            .unwrap()
    }

    fn remember_in(&self, text: &str, project: ProjectId) -> MemoryDto {
        self.memories
            .create(
                text,
                MemoryKind::Instruction,
                MemoryScopeKind::Project,
                Some(project),
                MemorySource::User,
                None,
            )
            .unwrap()
    }

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

fn edit(w: &World, entry: &MemoryDto, text: &str) {
    w.memories
        .update(
            entry.id,
            MemoryInput {
                text: Some(text.to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
}

#[tokio::test]
async fn a_chat_that_has_spoken_is_told_both_texts() {
    let w = world();
    let entry = w.remember("The user writes Rust, not Go.", MemoryKind::Instruction);
    let chat = w.m.create_chat(None).unwrap();
    w.pretend_it_spoke(chat.id);
    assert!(
        w.snapshot(chat.id).contains("writes Rust"),
        "it was frozen in"
    );

    edit(&w, &entry, "The user writes Rust and TypeScript.");

    // The prompt it was answering is untouched…
    assert!(
        w.snapshot(chat.id)
            .contains("The user writes Rust, not Go.")
    );
    // …and the note carries what to stop using as well as what is true now, because a chat
    // holding the old sentence needs both.
    let notes = w.notes(chat.id);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains("The user writes Rust, not Go."),
        "{notes:?}"
    );
    assert!(
        notes[0].contains("The user writes Rust and TypeScript."),
        "{notes:?}"
    );
}

#[tokio::test]
async fn a_chat_that_has_not_spoken_is_rebuilt_instead() {
    let w = world();
    let entry = w.remember("The user writes Rust, not Go.", MemoryKind::Instruction);
    let chat = w.m.create_chat(None).unwrap();

    edit(&w, &entry, "The user writes Rust and TypeScript.");

    let prompt = w.snapshot(chat.id);
    assert!(prompt.contains("Rust and TypeScript"), "{prompt}");
    assert!(
        !prompt.contains("not Go"),
        "the old text survived the rebuild"
    );
    assert!(
        w.notes(chat.id).is_empty(),
        "a rebuilt chat is not also told"
    );
}

#[tokio::test]
async fn a_deleted_memory_is_announced_with_its_text() {
    let w = world();
    let entry = w.remember(
        "The user's deploy target is fly.io.",
        MemoryKind::Preference,
    );
    let chat = w.m.create_chat(None).unwrap();
    w.pretend_it_spoke(chat.id);

    w.memories.archive(entry.id).unwrap();

    let notes = w.notes(chat.id);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("Forget this"), "{notes:?}");
    assert!(notes[0].contains("fly.io"), "{notes:?}");
}

/// The pair that has to hold together: a chat told to remember something is told when it goes.
/// An entry written after a chat froze its prompt is not in that chat's snapshot, so without
/// the record being kept up to date the chat would be given a sentence and never released from
/// it.
#[tokio::test]
async fn what_a_chat_was_told_to_remember_it_is_told_to_forget() {
    let w = world();
    let chat = w.m.create_chat(None).unwrap();
    w.pretend_it_spoke(chat.id);
    let entry = w.remember(
        "The user's deploy target is fly.io.",
        MemoryKind::Preference,
    );
    assert_eq!(w.notes(chat.id).len(), 1, "it was told to remember it");

    w.memories.archive(entry.id).unwrap();

    let notes = w.notes(chat.id);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(notes[1].contains("Forget this"), "{notes:?}");
    assert!(notes[1].contains("fly.io"), "{notes:?}");
}

#[tokio::test]
async fn a_new_long_tail_fact_tells_nobody() {
    let w = world();
    let chat = w.m.create_chat(None).unwrap();
    w.pretend_it_spoke(chat.id);

    w.remember("The user's cat is called Mikkel.", MemoryKind::Fact);

    // §B4: a fact is looked up per message. Announcing it would be a system note in every open
    // conversation for something the next turn finds by itself.
    assert!(w.notes(chat.id).is_empty(), "{:?}", w.notes(chat.id));
}

#[tokio::test]
async fn a_project_memory_stays_inside_its_project() {
    let w = world();
    let mine = w.project("Gantry");
    let theirs = w.project("Something else");
    let entry = w.remember_in("Answer in Norwegian.", mine);
    let inside =
        w.m.create_session(Surface::Chat, Vec::new(), None, false, Some(mine))
            .unwrap();
    let outside =
        w.m.create_session(Surface::Chat, Vec::new(), None, false, Some(theirs))
            .unwrap();
    w.pretend_it_spoke(inside.id);
    w.pretend_it_spoke(outside.id);

    edit(&w, &entry, "Answer in Norwegian, and keep it short.");

    assert_eq!(w.notes(inside.id).len(), 1, "{:?}", w.notes(inside.id));
    assert!(w.notes(outside.id).is_empty(), "{:?}", w.notes(outside.id));
}

#[tokio::test]
async fn an_incognito_chat_hears_nothing_about_memory() {
    let w = world();
    let entry = w.remember("The user writes Rust, not Go.", MemoryKind::Instruction);
    let private =
        w.m.create_session(Surface::Chat, Vec::new(), None, true, None)
            .unwrap();
    w.pretend_it_spoke(private.id);

    edit(&w, &entry, "The user writes Rust and TypeScript.");

    // 15 A21: it reads no memory and writes none, so a note about memory is the one thing its
    // whole promise is to do without.
    assert!(w.notes(private.id).is_empty(), "{:?}", w.notes(private.id));
}

#[tokio::test]
async fn the_chat_that_did_it_is_not_told_what_it_just_did() {
    let w = world();
    let entry = w.remember(
        "The user's deploy target is fly.io.",
        MemoryKind::Preference,
    );
    let asked = w.m.create_chat(None).unwrap();
    let other = w.m.create_chat(None).unwrap();
    w.pretend_it_spoke(asked.id);
    w.pretend_it_spoke(other.id);

    // What `propose_forget` does under auto-save, from inside a turn on `asked`.
    w.memories.archive_from(entry.id, Some(asked.id)).unwrap();

    assert!(w.notes(asked.id).is_empty(), "{:?}", w.notes(asked.id));
    assert_eq!(w.notes(other.id).len(), 1, "{:?}", w.notes(other.id));
}
