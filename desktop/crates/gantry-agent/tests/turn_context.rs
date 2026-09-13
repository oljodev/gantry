//! The per-turn context block (docs/plan/10 §5, 12 §A4, §B4): what a message pulls in, what it
//! does not pull in twice, and what the user's message ends up carrying.

use std::sync::Arc;

use gantry_agent::{
    ChatBook, NewChat, Skills,
    chats::TurnContextOptions,
    memory::{Memories, selector},
};
use gantry_core::{
    ContentPart, MemoryKind, MemoryScopeKind, MemorySource, Message, Mode, ModelRef,
    ReasoningEffort, SkillInput, SkillVersionSource, Surface,
};
use gantry_store::{BlobStore, Store};

struct World {
    _dir: tempfile::TempDir,
    book: ChatBook,
    skills: Arc<Skills>,
    memories: Arc<Memories>,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).unwrap());
    let book = ChatBook::new(store.clone(), blobs);
    let skills = Skills::new(store.clone(), dir.path().join("skills"));
    skills.rescan().unwrap();
    let memories = Memories::new(store);
    World {
        _dir: dir,
        book,
        skills,
        memories,
    }
}

impl World {
    fn chat(&self) -> gantry_core::ChatId {
        self.book
            .create(NewChat {
                surface: Surface::Chat,
                roots: Vec::new(),
                model: ModelRef::default_model(),
                mode: Mode::AutoEdit,
                guard: true,
                effort: ReasoningEffort::Off,
                system_snapshot: "sys".into(),
                system_snapshot_version: 1,
                connectors: Vec::new(),
                incognito: false,
                project: None,
                grants: Vec::new(),
            })
            .unwrap()
            .id
    }

    fn send(&self, chat: gantry_core::ChatId, text: &str) -> gantry_agent::chats::TurnInput {
        let input = self
            .book
            .begin_turn(
                chat,
                Message::user_text(text),
                Vec::new(),
                TurnContextOptions {
                    invoked: Vec::new(),
                    memory_on: true,
                },
            )
            .unwrap();
        self.book.finish_turn(
            chat,
            input.turn_id,
            gantry_agent::chats::TurnOutcome {
                status: gantry_core::TurnStatus::Completed,
                usage: None,
                stop_reason: Some(gantry_core::StopReason::EndTurn),
                error: None,
                tool_call_count: 0,
            },
        );
        input
    }
}

fn block_of(input: &gantry_agent::chats::TurnInput) -> Option<&str> {
    input.messages.last()?.parts.iter().find_map(|p| match p {
        ContentPart::TurnContext { text, .. } => Some(text.as_str()),
        _ => None,
    })
}

#[test]
fn a_matching_message_carries_the_skill_and_the_next_six_turns_do_not() {
    let w = world();
    let chat = w.chat();

    let input = w.send(chat, "write me a commit message for this change");
    let block = block_of(&input).expect("the commit-messages skill matched");
    assert!(block.starts_with("\n\n<gantry_turn_context>"), "{block}");
    assert!(block.contains("<skill name=\"commit-messages\" source=\"bundled\">"));
    assert!(
        block.contains("imperative"),
        "the body is there, not a stub"
    );
    assert_eq!(input.injected.skills.len(), 1);
    assert_eq!(input.injected.skills[0].how, "matched");

    // Asking again in the next turn sends nothing: the model still has it (12 §A4 rule 3).
    let again = w.send(chat, "another commit message please");
    assert!(block_of(&again).is_none(), "{:?}", block_of(&again));

    // And the counter on the skill only moved once.
    assert_eq!(
        w.skills.get("commit-messages").unwrap().unwrap().use_count,
        1
    );
}

#[test]
fn a_slash_invocation_beats_both_the_score_and_the_six_turn_rule() {
    let w = world();
    let chat = w.chat();
    w.send(chat, "write me a commit message");

    let input = w
        .book
        .begin_turn(
            chat,
            Message::user_text("do the thing"),
            Vec::new(),
            TurnContextOptions {
                invoked: vec!["commit-messages".to_owned()],
                memory_on: true,
            },
        )
        .unwrap();
    let block = block_of(&input).expect("the user asked for it by name");
    assert!(block.contains("commit-messages"));
    assert_eq!(input.injected.skills[0].how, "invoked");
}

#[test]
fn a_message_about_nothing_in_the_library_carries_no_block_at_all() {
    let w = world();
    let chat = w.chat();
    let input = w.send(chat, "what time is it in Oslo?");
    assert!(block_of(&input).is_none());
    assert!(input.injected.is_empty());
}

#[test]
fn a_fact_is_looked_up_by_the_words_of_the_message() {
    let w = world();
    let chat = w.chat();
    w.memories
        .create(
            "the API lives in services/api",
            MemoryKind::Fact,
            MemoryScopeKind::Global,
            None,
            MemorySource::User,
            None,
        )
        .unwrap();
    w.memories
        .create(
            "the office cat is called Mons",
            MemoryKind::Fact,
            MemoryScopeKind::Global,
            None,
            MemorySource::User,
            None,
        )
        .unwrap();

    let input = w.send(chat, "where does the api live?");
    let block = block_of(&input).expect("the fact matched");
    assert!(block.contains("services/api"), "{block}");
    assert!(
        !block.contains("Mons"),
        "only what the message is about: {block}"
    );
    assert_eq!(input.injected.memories.len(), 1);
    // Reaching a prompt is what "used" means (12 §B4).
    let used = w
        .memories
        .search(None, "services", 5)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(used.use_count, 1);
}

#[test]
fn pausing_memory_stops_the_lookup_and_leaves_skills_alone() {
    let w = world();
    let chat = w.chat();
    w.memories
        .create(
            "the API lives in services/api",
            MemoryKind::Fact,
            MemoryScopeKind::Global,
            None,
            MemorySource::User,
            None,
        )
        .unwrap();

    let input = w
        .book
        .begin_turn(
            chat,
            Message::user_text("where does the api live, and write a commit message"),
            Vec::new(),
            TurnContextOptions {
                invoked: Vec::new(),
                memory_on: false,
            },
        )
        .unwrap();
    let block = block_of(&input).expect("the skill still matched");
    assert!(block.contains("commit-messages"));
    assert!(
        !block.contains("services/api"),
        "paused means paused: {block}"
    );
    assert!(input.injected.memories.is_empty());
}

#[test]
fn a_skill_the_user_wrote_is_matched_beside_the_bundled_ones() {
    let w = world();
    let chat = w.chat();
    w.skills
        .save(
            &SkillInput {
                name: "sourdough".into(),
                description: "How this kitchen bakes. Use when baking bread or feeding a starter."
                    .into(),
                triggers: vec!["sourdough".into(), "starter".into(), "bread".into()],
                body: "# Sourdough\n\nFeed it twice a day.".into(),
                ..SkillInput::default()
            },
            SkillVersionSource::UserEdit,
        )
        .unwrap();

    let input = w.send(chat, "my sourdough starter smells wrong");
    let block = block_of(&input).expect("a user skill matches like any other");
    assert!(
        block.contains("<skill name=\"sourdough\" source=\"user\">"),
        "{block}"
    );
    assert!(block.contains("Feed it twice a day."));
}

#[test]
fn the_core_set_is_what_a_new_chat_freezes_and_the_long_tail_is_not() {
    let w = world();
    w.memories
        .create(
            "answer in Norwegian",
            MemoryKind::Instruction,
            MemoryScopeKind::Global,
            None,
            MemorySource::User,
            None,
        )
        .unwrap();
    w.memories
        .create(
            "the API lives in services/api",
            MemoryKind::Fact,
            MemoryScopeKind::Global,
            None,
            MemorySource::User,
            None,
        )
        .unwrap();

    let entries = w.memories.list(Default::default(), "").unwrap();
    let standing: Vec<_> = entries
        .into_iter()
        .filter(|m| m.kind.is_standing())
        .collect();
    let (block, ids) = selector::core_block(&standing);
    assert!(block.contains("- instruction: answer in Norwegian"));
    assert!(
        !block.contains("services/api"),
        "a fact is not standing behaviour"
    );
    assert_eq!(ids.len(), 1);
}
