//! The types Gantry ships with (docs/plan/18 §3, A4).
//!
//! In code rather than in migration 0016, which is where they started. **Reset** needs to know
//! what the original said, and a second copy of the same paragraph inside a SQL file is a second
//! copy to keep in step — so the migration makes the table and this fills it, at every start,
//! for whatever is missing.
//!
//! Two of them, one of each kind: `researcher` decides everything about itself, and `agent`
//! decides nothing but its own description. One of each is what teaches the distinction; a
//! library of five nobody asked for is five things to maintain and a longer tool description in
//! every request of every turn.

use gantry_core::{AgentModel, AgentType, INHERIT, Mode, OpenField};
use gantry_store::{Store, repos};

/// The built-in types, exactly as they ship.
#[must_use]
pub fn builtins() -> Vec<AgentType> {
    vec![
        AgentType {
            id: "researcher".to_owned(),
            name: "Researcher".to_owned(),
            description: "Reads the web and reports back. Give it a question and say what you \
                          need out of it. It cannot change anything."
                .to_owned(),
            instructions: RESEARCHER.to_owned(),
            model: AgentModel::Inherit,
            connectors: vec!["web".to_owned()],
            // Auto with the guard: it can only read, so stopping the user to confirm a page
            // fetch would be a card about nothing. The chat that started it still narrows this.
            mode: Some(Mode::Auto),
            guard: Some(true),
            write_files: false,
            memory: false,
            skills: false,
            open: Vec::new(),
            builtin: true,
            enabled: true,
        },
        AgentType {
            id: "agent".to_owned(),
            name: "General agent".to_owned(),
            description: "A sub agent you brief yourself: write its instructions and say which \
                          tools it needs. Use it for work that is not research."
                .to_owned(),
            instructions: GENERAL.to_owned(),
            model: AgentModel::Rules,
            connectors: vec![INHERIT.to_owned()],
            mode: None,
            guard: None,
            write_files: false,
            memory: false,
            skills: false,
            open: vec![
                OpenField::Instructions,
                OpenField::Connectors,
                OpenField::Write,
                OpenField::Model,
            ],
            builtin: true,
            enabled: true,
        },
    ]
}

const RESEARCHER: &str = "You are a research sub agent. You were given one question by another \
model and your whole output is the answer to it.

Read before you answer. Search, then open the promising results with fetch_url and read them; a \
snippet is not a source. Use find_in_page on a long page rather than paging through it from the \
top. Quote what a page actually says, with the address you read it at, and say plainly when you \
could not find something rather than filling the gap from memory.

Answer the question you were given and nothing beside it. No preamble, no offer to continue, no \
questions back — nobody will read a question. Lead with the answer, then the evidence for it, \
then anything you could not settle.";

const GENERAL: &str = "You are a sub agent. Another model gave you the task below and will read \
your report; the user will not see this conversation and cannot answer you.

Do the work, then report: what you did, what you found, and anything the model that briefed you \
has to decide. No preamble and no questions back.";

/// Writes any built-in the library does not have yet. Called at startup, and a no-op on every
/// run after the first.
///
/// Missing rather than outdated: a user who has edited `researcher` keeps their edit through
/// every release, and **Reset** is how they ask for the original back.
pub fn seed(store: &Store) {
    let existing = match store.read(repos::agents::list) {
        Ok(list) => list,
        Err(err) => {
            log::warn!("could not read the sub-agent library: {err}");
            return;
        }
    };
    for builtin in builtins() {
        if existing.iter().any(|a| a.id == builtin.id) {
            continue;
        }
        if let Err(err) = store.write_blocking(move |c| repos::agents::upsert(c, &builtin)) {
            log::warn!("could not add a built-in sub agent: {err}");
        }
    }
}

/// What a saved type has to be before it reaches the table.
///
/// Here rather than in the command because the rules are about the type, not about the dialog:
/// the id is what a model types in a call, and the description is what it reads when choosing
/// between types, so an empty one makes a sub agent nothing can pick on purpose.
pub fn validate(agent: &AgentType) -> Result<(), String> {
    let id = agent.id.trim();
    if id.is_empty() {
        return Err("a sub agent needs an id".to_owned());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(
            "an id is lowercase letters, digits and hyphens: it is what the model types".to_owned(),
        );
    }
    if agent.name.trim().is_empty() {
        return Err("a sub agent needs a name".to_owned());
    }
    if agent.description.trim().is_empty() {
        return Err(
            "a sub agent needs a description — it is what the model reads when it chooses \
             between them"
                .to_owned(),
        );
    }
    Ok(())
}

/// What a built-in looked like when it shipped, for **Reset**.
#[must_use]
pub fn original(id: &str) -> Option<AgentType> {
    builtins().into_iter().find(|a| a.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_ships_is_one_type_of_each_kind() {
        let all = builtins();
        assert_eq!(all.len(), 2);
        let researcher = original("researcher").unwrap();
        assert!(
            researcher.open.is_empty(),
            "it knows its job and settles every field"
        );
        let general = original("agent").unwrap();
        assert_eq!(
            general.open.len(),
            4,
            "it settles nothing but its own description"
        );
        assert!(all.iter().all(|a| validate(a).is_ok()));
    }

    #[test]
    fn a_fresh_library_is_filled_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("t.db")).unwrap();
        assert!(store.read(repos::agents::list).unwrap().is_empty());

        seed(&store);
        let ids: Vec<String> = store
            .read(repos::agents::list)
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect();
        assert_eq!(ids, vec!["agent".to_owned(), "researcher".to_owned()]);

        // An edit survives every later start: seeding fills what is missing, never what is
        // there, and Reset is how the original is asked for back.
        let mut edited = original("researcher").unwrap();
        edited.instructions = "mine".into();
        store
            .write_blocking(move |c| repos::agents::upsert(c, &edited))
            .unwrap();
        seed(&store);
        assert_eq!(
            store
                .read(|c| repos::agents::get(c, "researcher"))
                .unwrap()
                .unwrap()
                .instructions,
            "mine"
        );
    }

    #[test]
    fn an_id_is_what_a_model_has_to_type() {
        let mut agent = original("agent").unwrap();
        agent.id = "My Agent".into();
        let err = validate(&agent).unwrap_err();
        assert!(err.contains("lowercase"), "{err}");

        agent.id = "my-agent".into();
        assert!(validate(&agent).is_ok());

        // A type nothing can choose on purpose is worse than no type at all.
        agent.description = "  ".into();
        assert!(validate(&agent).unwrap_err().contains("description"));
    }
}
