//! The skills service (docs/plan/12 §A3, §A5): the index against the files, and the rules
//! that make a bundled skill different from one the user wrote.

use std::sync::Arc;

use gantry_agent::Skills;
use gantry_core::{SkillInput, SkillSource, SkillVersionSource};
use gantry_store::Store;

fn skills() -> (tempfile::TempDir, Arc<Skills>) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let skills = Skills::new(store, dir.path().join("skills"));
    (dir, skills)
}

#[test]
fn the_bundled_set_indexes_itself_on_the_first_scan() {
    let (_dir, skills) = skills();
    skills.rescan().unwrap();
    let all = skills.list().unwrap();
    assert!(all.len() >= 4, "the starter set is there: {all:?}");
    assert!(all.iter().all(|s| s.source == SkillSource::Bundled));
    assert!(all.iter().all(|s| !s.description.is_empty()));

    let authoring = all
        .iter()
        .find(|s| s.id == "artifact-authoring")
        .expect("the artifact guide ships");
    assert_eq!(authoring.references, ["react-runtime.md"]);
    assert!(
        skills
            .reference("artifact-authoring", "react-runtime.md")
            .unwrap()
            .contains("recharts"),
        "a bundled reference is readable out of the binary"
    );

    // Rescanning changes nothing and loses nothing.
    skills.rescan().unwrap();
    assert_eq!(skills.list().unwrap().len(), all.len());
}

#[test]
fn a_bundled_skill_can_be_switched_off_but_not_edited_or_deleted() {
    let (_dir, skills) = skills();
    skills.rescan().unwrap();

    let err = skills.delete("code-review").unwrap_err();
    assert!(format!("{err:?}").contains("switched off"), "{err:?}");

    let err = skills
        .save(
            &SkillInput {
                name: "code-review".into(),
                description: "Mine now.".into(),
                body: "Body.".into(),
                ..SkillInput::default()
            },
            SkillVersionSource::UserEdit,
        )
        .unwrap_err();
    assert!(format!("{err:?}").contains("ships with Gantry"), "{err:?}");

    skills.set_enabled("code-review", false).unwrap();
    assert!(!skills.get("code-review").unwrap().unwrap().enabled);
    // And the switch survives the next scan, because it is ours and not the file's.
    skills.rescan().unwrap();
    assert!(!skills.get("code-review").unwrap().unwrap().enabled);
}

#[test]
fn saving_writes_a_file_a_person_could_have_written() {
    let (_dir, skills) = skills();
    skills.rescan().unwrap();
    let saved = skills
        .save(
            &SkillInput {
                name: "rust-idioms".into(),
                description: "Idiomatic Rust. Use when writing Rust.".into(),
                triggers: vec!["rust".into(), "borrow checker".into()],
                body: "# Rust\n\nPrefer ownership.".into(),
                ..SkillInput::default()
            },
            SkillVersionSource::UserEdit,
        )
        .unwrap();
    assert_eq!(saved.version, 1);
    assert_eq!(saved.source, SkillSource::User);

    let path = std::path::Path::new(saved.path.as_ref().unwrap()).join("SKILL.md");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("---\nname: rust-idioms\n"), "{text}");
    assert!(
        text.contains("gantry-triggers: \"rust, borrow checker\""),
        "{text}"
    );

    // A second save is version 2, and version 1 is still readable.
    let again = skills
        .save(
            &SkillInput {
                name: "rust-idioms".into(),
                description: "Idiomatic Rust, revised.".into(),
                body: "# Rust\n\nRevised.".into(),
                ..SkillInput::default()
            },
            SkillVersionSource::UserEdit,
        )
        .unwrap();
    assert_eq!(again.version, 2);
    assert_eq!(
        skills.detail("rust-idioms").unwrap().body.trim(),
        "# Rust\n\nRevised."
    );
}

#[test]
fn a_file_edited_outside_gantry_is_picked_up_and_snapshotted() {
    let (_dir, skills) = skills();
    skills.rescan().unwrap();
    let saved = skills
        .save(
            &SkillInput {
                name: "notes".into(),
                description: "Notes. Use when taking notes.".into(),
                body: "First.".into(),
                ..SkillInput::default()
            },
            SkillVersionSource::UserEdit,
        )
        .unwrap();

    let path = std::path::Path::new(saved.path.as_ref().unwrap()).join("SKILL.md");
    std::fs::write(
        &path,
        "---\nname: notes\ndescription: Rewritten by hand in another editor.\n---\nSecond.\n",
    )
    .unwrap();

    skills.rescan().unwrap();
    let after = skills.get("notes").unwrap().unwrap();
    assert_eq!(after.description, "Rewritten by hand in another editor.");
    assert!(after.version > saved.version, "the change is a new version");
    assert_eq!(skills.detail("notes").unwrap().body.trim(), "Second.");
}

#[test]
fn a_folder_that_is_gone_leaves_the_index() {
    let (_dir, skills) = skills();
    skills.rescan().unwrap();
    let saved = skills
        .save(
            &SkillInput {
                name: "temporary".into(),
                description: "Here for a moment.".into(),
                body: "Body.".into(),
                ..SkillInput::default()
            },
            SkillVersionSource::UserEdit,
        )
        .unwrap();
    std::fs::remove_dir_all(saved.path.as_ref().unwrap()).unwrap();
    skills.rescan().unwrap();
    assert!(skills.get("temporary").unwrap().is_none());
}

#[test]
fn the_inventory_names_what_is_there_and_says_how_to_get_the_rest() {
    let (_dir, skills) = skills();
    skills.rescan().unwrap();
    let inventory = skills.inventory(&[]).unwrap();
    assert!(inventory.starts_with("<gantry_skills>"));
    assert!(inventory.contains("commit-messages — "));
    assert!(inventory.contains("gantry__load_skill"));
    // A switched-off skill is not offered.
    skills.set_enabled("commit-messages", false).unwrap();
    assert!(!skills.inventory(&[]).unwrap().contains("commit-messages"));
}

#[test]
fn a_free_name_is_suggested_when_one_is_taken() {
    let (_dir, skills) = skills();
    skills.rescan().unwrap();
    assert_eq!(skills.free_name("brand-new").unwrap(), "brand-new");
    assert_eq!(skills.free_name("code-review").unwrap(), "code-review-2");
}
