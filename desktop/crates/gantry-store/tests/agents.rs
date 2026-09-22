//! The `agent_types` table (docs/plan/18 §3): what a round trip through it keeps, and the
//! retention sweep beside it.
//!
//! What Gantry *ships* is tested in `gantry-agent`, which is where the built-ins live: the
//! migration makes the table and `subagents::library::seed` fills it.

use gantry_core::{AgentModel, AgentType, Mode, OpenField};
use gantry_store::{Store, repos::agents};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("t.db")).unwrap();
    (dir, store)
}

#[test]
fn a_type_survives_the_round_trip_and_a_built_in_cannot_be_deleted() {
    let (_dir, store) = store();
    let mine = AgentType {
        id: "reviewer".into(),
        name: "Reviewer".into(),
        description: "Reads a diff and argues with it.".into(),
        instructions: "Look for the bug the author would be embarrassed by.".into(),
        model: AgentModel::Named {
            model: gantry_core::ModelRef {
                provider: gantry_core::ProviderId::new("anthropic".to_owned()),
                model: "claude-sonnet-5".to_owned(),
            },
        },
        connectors: vec!["filesystem".into()],
        mode: Some(Mode::Plan),
        guard: Some(false),
        write_files: false,
        memory: true,
        skills: true,
        open: vec![OpenField::Instructions, OpenField::Write],
        builtin: false,
        enabled: true,
    };
    store
        .write_blocking({
            let mine = mine.clone();
            move |c| agents::upsert(c, &mine)
        })
        .unwrap();
    let read = store
        .read(|c| agents::get(c, "reviewer"))
        .unwrap()
        .expect("it is in the library");
    assert_eq!(read, mine, "every field, including which ones are open");

    // A built-in is switched off, never deleted, so Reset has something to reset to (18 §3).
    let shipped = AgentType {
        id: "researcher".into(),
        builtin: true,
        ..mine.clone()
    };
    store
        .write_blocking(move |c| agents::upsert(c, &shipped))
        .unwrap();
    store
        .write_blocking(|c| agents::remove(c, "researcher"))
        .unwrap();
    assert!(
        store
            .read(|c| agents::get(c, "researcher"))
            .unwrap()
            .is_some()
    );
    store
        .write_blocking(|c| agents::remove(c, "reviewer"))
        .unwrap();
    assert!(
        store
            .read(|c| agents::get(c, "reviewer"))
            .unwrap()
            .is_none()
    );
}

/// The retention setting (18 §9): a sub agent's transcript can be swept by age, and nothing
/// else ever is.
#[test]
fn only_sub_agent_transcripts_are_swept_by_age() {
    use gantry_core::{
        ChatId, Mode, ModelRef, ProviderId, ReasoningEffort, Surface, TurnId, TurnStatus,
    };
    use gantry_store::repos::{chats, turns};

    let (_dir, store) = store();
    let day = 24 * 60 * 60 * 1000;
    let chat = |parent: Option<TurnId>, last: i64| chats::ChatRecord {
        id: ChatId::new(),
        surface: Surface::Chat,
        project_id: None,
        title: "T".into(),
        title_source: "auto".into(),
        pinned: false,
        mode: Mode::AutoEdit,
        guard: true,
        model: ModelRef::new(ProviderId::openrouter(), "test/model"),
        effort: ReasoningEffort::Off,
        web_search: false,
        instructions: String::new(),
        system_snapshot: String::new(),
        system_snapshot_version: 1,
        created_at: last,
        updated_at: last,
        last_message_at: last,
        archived_at: None,
        incognito: false,
        parent_turn_id: parent,
        agent_type: parent.map(|_| "researcher".to_owned()),
    };

    let (parent, old, young) = store
        .write_blocking(move |c| {
            let parent = chat(None, 0);
            chats::insert(c, &parent)?;
            let turn = turns::TurnRecord {
                id: TurnId::new(),
                chat_id: parent.id,
                seq: 1,
                status: TurnStatus::Completed,
                model: parent.model.clone(),
                started_at: 0,
                ended_at: Some(0),
                usage: None,
                stop_reason: None,
                error: None,
                feedback: None,
                tool_call_count: 0,
            };
            turns::insert(c, &turn)?;
            let old = chat(Some(turn.id), gantry_core::now_ms() - 40 * day);
            let young = chat(Some(turn.id), gantry_core::now_ms() - 2 * day);
            chats::insert(c, &old)?;
            chats::insert(c, &young)?;
            Ok((parent.id, old.id, young.id))
        })
        .unwrap();

    let swept = store
        .write_blocking(move |c| chats::delete_old_sub_agents(c, gantry_core::now_ms() - 30 * day))
        .unwrap();
    assert_eq!(swept, 1);
    assert!(store.read(move |c| chats::get(c, old)).unwrap().is_none());
    assert!(store.read(move |c| chats::get(c, young)).unwrap().is_some());
    // The conversation a person had is never in that query, whatever its age.
    assert!(
        store
            .read(move |c| chats::get(c, parent))
            .unwrap()
            .is_some()
    );
}
