//! Round trips through the chat tables of migration 0002, search and crash recovery.

use gantry_core::{
    ChatId, ContentPart, Feedback, Message, Mode, ModelRef, ReasoningEffort, Role, StopReason,
    TurnId, TurnStatus, Usage, now_ms,
};
use gantry_store::{
    Store,
    repos::{chats, messages, search, turns},
};

fn open() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("t.db")).unwrap();
    (dir, store)
}

fn chat(title: &str) -> chats::ChatRecord {
    let now = now_ms();
    chats::ChatRecord {
        id: ChatId::new(),
        project_id: None,
        title: title.into(),
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
    }
}

fn turn(chat_id: ChatId, seq: u32) -> turns::TurnRecord {
    turns::TurnRecord {
        id: TurnId::new(),
        chat_id,
        seq,
        status: TurnStatus::Running,
        model: ModelRef::default_model(),
        started_at: now_ms(),
        ended_at: None,
        usage: None,
        stop_reason: None,
        error: None,
        feedback: None,
        tool_call_count: 0,
    }
}

#[test]
fn a_chat_with_a_turn_round_trips() {
    let (_dir, store) = open();
    let c = chat("WAL mode in SQLite");
    let t = turn(c.id, 1);
    let (c2, t2) = (c.clone(), t.clone());
    store
        .write_blocking(move |conn| {
            chats::insert(conn, &c2)?;
            turns::insert(conn, &t2)?;
            messages::insert(
                conn,
                &messages::MessageRecord {
                    message: Message::user_text("Explain WAL mode"),
                    chat_id: c2.id,
                    turn_id: Some(t2.id),
                    seq: 1,
                    stop_reason: None,
                    usage: None,
                },
            )?;
            let mut a = Message::user_text("It is a journal mode.");
            a.role = Role::Assistant;
            a.parts.insert(
                0,
                ContentPart::Thinking {
                    text: "hm".into(),
                    signature: None,
                    provider: gantry_core::ProviderKind::OpenAiChat,
                },
            );
            messages::insert(
                conn,
                &messages::MessageRecord {
                    message: a,
                    chat_id: c2.id,
                    turn_id: Some(t2.id),
                    seq: 2,
                    stop_reason: Some(StopReason::EndTurn),
                    usage: Some(Usage {
                        input: 3,
                        output: 4,
                        ..Default::default()
                    }),
                },
            )?;
            let mut done = t2.clone();
            done.status = TurnStatus::Completed;
            done.ended_at = Some(now_ms());
            done.usage = Some(Usage {
                input: 3,
                output: 4,
                ..Default::default()
            });
            done.stop_reason = Some(StopReason::EndTurn);
            done.feedback = Some(Feedback::Good);
            turns::update(conn, &done)?;
            Ok(())
        })
        .unwrap();

    let got = store.read(|conn| chats::get(conn, c.id)).unwrap().unwrap();
    assert_eq!(got.title, "WAL mode in SQLite");
    assert_eq!(got.mode, Mode::AutoEdit);
    assert_eq!(got.model, ModelRef::default_model());

    let ts = store.read(|conn| turns::list_for_chat(conn, c.id)).unwrap();
    assert_eq!(ts.len(), 1);
    assert_eq!(ts[0].status, TurnStatus::Completed);
    assert_eq!(ts[0].feedback, Some(Feedback::Good));
    assert_eq!(ts[0].usage.unwrap().output, 4);
    assert_eq!(ts[0].stop_reason, Some(StopReason::EndTurn));

    let ms = store
        .read(|conn| messages::list_for_chat(conn, c.id))
        .unwrap();
    assert_eq!(ms.len(), 2);
    assert_eq!(ms[0].message.role, Role::User);
    assert_eq!(ms[1].message.parts.len(), 2);
    assert_eq!(ms[1].message.text(), "It is a journal mode.");
    assert_eq!(store.read(|conn| turns::next_seq(conn, c.id)).unwrap(), 2);
    assert_eq!(
        store.read(|conn| messages::next_seq(conn, c.id)).unwrap(),
        3
    );

    // Search finds the chat by title and the messages by text, with a snippet.
    let hits = store.read(|conn| search::search(conn, "wal", 10)).unwrap();
    assert_eq!(hits.len(), 2, "{hits:?}");
    assert_eq!(hits[0].kind, gantry_core::SearchHitKind::Chat);
    assert_eq!(hits[1].kind, gantry_core::SearchHitKind::Message);
    assert!(hits[1].snippet.contains("WAL"));
    let hits = store
        .read(|conn| search::search(conn, "journal mo", 10))
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(
        store
            .read(|conn| search::search(conn, "\"", 10))
            .unwrap()
            .is_empty()
    );

    // Deleting the chat cascades.
    assert!(
        store
            .write_blocking(move |conn| chats::delete(conn, c.id))
            .unwrap()
    );
    assert!(
        store
            .read(|conn| turns::list_for_chat(conn, c.id))
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .read(|conn| messages::list_for_chat(conn, c.id))
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .read(|conn| search::search(conn, "wal", 10))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn running_turns_are_interrupted_at_startup_and_titles_respect_the_user() {
    let (_dir, store) = open();
    let c = chat("first words");
    let t = turn(c.id, 1);
    let (c2, t2) = (c.clone(), t.clone());
    store
        .write_blocking(move |conn| {
            chats::insert(conn, &c2)?;
            turns::insert(conn, &t2)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        store
            .read(|conn| turns::chats_with_running_turns(conn))
            .unwrap(),
        vec![(c.id, t.id)]
    );
    let n = store
        .write_blocking(|conn| turns::interrupt_running(conn, 42))
        .unwrap();
    assert_eq!(n, 1);
    let got = store.read(|conn| turns::get(conn, t.id)).unwrap().unwrap();
    assert_eq!(got.status, TurnStatus::Interrupted);
    assert_eq!(got.ended_at, Some(42));
    assert!(got.error.unwrap().contains("closed"));

    assert!(
        store
            .write_blocking(move |conn| chats::set_auto_title(conn, c.id, "Better"))
            .unwrap()
    );
    let mut renamed = store.read(|conn| chats::get(conn, c.id)).unwrap().unwrap();
    assert_eq!(renamed.title, "Better");
    renamed.title = "Mine".into();
    renamed.title_source = "user".into();
    store
        .write_blocking(move |conn| chats::update(conn, &renamed))
        .unwrap();
    assert!(
        !store
            .write_blocking(move |conn| chats::set_auto_title(conn, c.id, "Nope"))
            .unwrap()
    );
    assert_eq!(
        store
            .read(|conn| chats::get(conn, c.id))
            .unwrap()
            .unwrap()
            .title,
        "Mine"
    );
}

#[test]
fn backups_exclude_credentials() {
    let (dir, store) = open();
    store
        .write_blocking(|conn| {
            conn.execute(
                "INSERT INTO credentials (id, kind, owner_kind, owner_id, ciphertext, nonce, created_at, updated_at)
                 VALUES ('c1', 'api_key', 'provider', 'openrouter', x'00', x'00', 0, 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let dest = dir.path().join("backup.db");
    store.backup_to(&dest).unwrap();
    let copy = gantry_store::Connection::open(&dest).unwrap();
    let n: i64 = copy
        .query_row("SELECT count(*) FROM credentials", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    let n: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM credentials", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 1, "the live database keeps its rows");
    store.integrity_check().unwrap();
}
