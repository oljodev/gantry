//! The sub-agent library (docs/plan/18 §3): what migration 0016 ships and what a round trip
//! through the table keeps.

use gantry_core::{AgentModel, AgentType, Mode, OpenField};
use gantry_store::{Store, repos::agents};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("t.db")).unwrap();
    (dir, store)
}

#[test]
fn two_types_ship_and_they_decide_different_things() {
    let (_dir, store) = store();
    let library = store.read(agents::list).unwrap();
    let ids: Vec<&str> = library.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec!["agent", "researcher"], "by name");

    let researcher = library.iter().find(|a| a.id == "researcher").unwrap();
    // It knows its job: nothing about it is the caller's to change (18 A4).
    assert!(researcher.open.is_empty());
    assert_eq!(researcher.connectors, vec!["web".to_owned()]);
    assert!(
        !researcher.write_files,
        "it reports, it does not change things"
    );
    assert_eq!(researcher.mode, Some(Mode::Auto));
    assert!(researcher.instructions.contains("Read before you answer"));

    // The general one knows nothing until it is told, which is what a coding task wants.
    let general = library.iter().find(|a| a.id == "agent").unwrap();
    assert!(general.opens(OpenField::Instructions));
    assert!(general.opens(OpenField::Connectors));
    assert!(general.opens(OpenField::Write));
    assert!(general.opens(OpenField::Model));
    assert_eq!(general.connectors, vec![gantry_core::INHERIT.to_owned()]);
    assert_eq!(
        general.mode, None,
        "it runs as the chat that started it does"
    );

    assert!(library.iter().all(|a| a.builtin && a.enabled));
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
