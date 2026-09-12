//! The memory tools (docs/plan/12 §B3, §B7): what the model may write, what it may remove, and
//! what it is stopped from doing in either direction.

use std::sync::{Arc, RwLock};

use gantry_agent::{Memories, interactions::Interactions, runtime_tools::memory as mem_tools};
use gantry_connectors::{ChatScope, NoopToolEvents, ToolCallRequest, ToolEventSink, ToolOutcome};
use gantry_core::{
    CallId, ChatId, MemoryKind, MemoryScopeKind, MemorySource, Mode, Settings, TurnId, memory,
};
use gantry_store::Store;
use mem_tools::MemoryTools;
use serde_json::json;

struct World {
    _dir: tempfile::TempDir,
    tools: Arc<MemoryTools>,
    memories: Arc<Memories>,
    sink: Arc<dyn ToolEventSink>,
    turn: TurnId,
    chat: ChatId,
}

fn world(auto_save: bool) -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let memories = Memories::new(store);
    let mut settings = Settings::default();
    settings.memory.auto_save_global = auto_save;
    settings.memory.auto_save_project = auto_save;
    let tools = MemoryTools::new(
        memories.clone(),
        Interactions::new(),
        Arc::new(RwLock::new(settings)),
    );
    World {
        _dir: dir,
        tools,
        memories,
        sink: Arc::new(NoopToolEvents),
        turn: TurnId::new(),
        chat: ChatId::new(),
    }
}

impl World {
    fn call(&self, tool: &str, args: serde_json::Value) -> ToolOutcome {
        self.tools.call(
            &ToolCallRequest {
                call_id: CallId::new(),
                tool: tool.to_owned(),
                args,
                scope: ChatScope {
                    chat_id: self.chat,
                    turn_id: self.turn,
                    mode: Mode::AutoEdit,
                    attach_decided: true,
                },
            },
            &self.sink,
        )
    }

    fn live(&self) -> Vec<String> {
        self.memories
            .list(Default::default(), "")
            .unwrap()
            .into_iter()
            .map(|m| m.text)
            .collect()
    }

    fn remember(&self, text: &str) -> gantry_core::MemoryId {
        self.memories
            .create(
                text,
                MemoryKind::Preference,
                MemoryScopeKind::Global,
                None,
                MemorySource::User,
                None,
            )
            .unwrap()
            .id
    }
}

fn structured(outcome: &ToolOutcome) -> serde_json::Value {
    let ToolOutcome::Complete { structured, .. } = outcome;
    structured.clone().unwrap_or(serde_json::Value::Null)
}

fn is_error(outcome: &ToolOutcome) -> bool {
    let ToolOutcome::Complete { is_error, .. } = outcome;
    *is_error
}

#[test]
fn auto_save_writes_the_entry_and_says_so() {
    let w = world(true);
    let out = w.call(
        mem_tools::PROPOSE,
        json!({ "text": "Prefers Rust over Python", "kind": "preference", "reason": "They said so." }),
    );
    assert_eq!(structured(&out)["status"], "saved");
    assert_eq!(w.live(), vec!["Prefers Rust over Python".to_owned()]);
}

#[test]
fn without_auto_save_nothing_is_written_until_the_user_answers() {
    let w = world(false);
    let out = w.call(
        mem_tools::PROPOSE,
        json!({ "text": "Prefers Rust over Python", "kind": "preference", "reason": "They said so." }),
    );
    assert_eq!(structured(&out)["status"], "proposed");
    assert!(w.live().is_empty());
}

#[test]
fn a_replacement_archives_what_it_replaces() {
    let w = world(true);
    let old = w.remember("Prefers npm");
    let out = w.call(
        mem_tools::PROPOSE,
        json!({
            "text": "Prefers pnpm, never npm",
            "kind": "preference",
            "reason": "They corrected it.",
            "replaces_id": old.to_string(),
        }),
    );
    assert_eq!(structured(&out)["status"], "saved");
    assert_eq!(w.live(), vec!["Prefers pnpm, never npm".to_owned()]);
}

#[test]
fn auto_save_forgets_straight_away_and_leaves_it_restorable() {
    let w = world(true);
    let id = w.remember("The API lives in services/api");
    let out = w.call(
        mem_tools::FORGET,
        json!({ "memory_id": id.to_string(), "reason": "It moved." }),
    );
    assert_eq!(structured(&out)["status"], "forgotten");
    assert!(w.live().is_empty());
    assert!(w.memories.get(id).unwrap().unwrap().archived_at.is_some());
    w.memories.restore(id).unwrap();
    assert_eq!(w.live().len(), 1);
}

#[test]
fn a_secret_is_never_remembered() {
    let w = world(true);
    let out = w.call(
        mem_tools::PROPOSE,
        json!({
            "text": "The key is sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJKKKKLLLL",
            "kind": "fact",
            "reason": "They pasted it.",
        }),
    );
    assert!(is_error(&out));
    assert!(w.live().is_empty());
}

#[test]
fn forgetting_has_its_own_larger_budget() {
    let w = world(true);
    let ids: Vec<_> = (0..8)
        .map(|i| w.remember(&format!("Stale note {i}")))
        .collect();
    let mut forgotten = 0;
    for id in &ids {
        let out = w.call(
            mem_tools::FORGET,
            json!({ "memory_id": id.to_string(), "reason": "Stale." }),
        );
        if !is_error(&out) {
            forgotten += 1;
        }
    }
    assert_eq!(forgotten, memory::MAX_FORGETS_PER_TURN as usize);
    // Remembering keeps its own, smaller budget in the same turn.
    let mut written = 0;
    for i in 0..4 {
        let out = w.call(
            mem_tools::PROPOSE,
            json!({ "text": format!("A standing preference number {i}"), "kind": "preference", "reason": "Said so." }),
        );
        if !is_error(&out) {
            written += 1;
        }
    }
    assert_eq!(written, memory::MAX_PROPOSALS_PER_TURN as usize);
}

#[test]
fn search_without_a_query_reads_the_whole_store() {
    let w = world(true);
    for i in 0..5 {
        w.remember(&format!("Entry number {i}"));
    }
    let out = w.call(mem_tools::SEARCH, json!({}));
    assert_eq!(structured(&out)["memories"].as_array().unwrap().len(), 5);
}
