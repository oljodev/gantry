//! `gantry__update_todos`: a checklist the model keeps for a task of several steps, and the user
//! watches it work through (docs/plan/03 §9b).
//!
//! **The list has no store.** Each call carries the whole list, and the list *is* the arguments
//! of the latest call: the transcript already keeps those, replays them to the model, and hands
//! them to the interface, which draws the newest one as the turn's plan. A table beside it would
//! be a second copy of the same fact with its own ways of disagreeing. What the tool does is
//! check the list is one a person can follow — no empty items, one thing in progress at a time —
//! and say back where things stand, in a sentence rather than an echo of what the model just
//! wrote.
//!
//! It is `app` tier (04 §2): it touches nothing but what is on screen, so it never asks, and Plan
//! mode keeps it, which is where a list of steps is most at home.

use gantry_connectors::ToolOutcome;
use gantry_core::{ResultPart, RiskTier, ToolDef};
use serde::Deserialize;
use serde_json::{Value, json};

pub const NAME: &str = "update_todos";

/// More than this is not a checklist anybody reads, and a model that writes one is usually
/// listing files rather than steps.
const MAX_ITEMS: usize = 40;
/// One line on screen. The work itself belongs in the steps, not in the item that names them.
const MAX_CHARS: usize = 200;

#[must_use]
pub fn definition() -> ToolDef {
    ToolDef::new(
        NAME,
        "Keep a checklist of the steps of the task you are doing. The user sees the newest list \
         beside the conversation while you work, so it is how they follow a long task. Each call \
         replaces the whole list: send every item, each time.\n\n\
         Use it when a task has three or more distinct steps, or when the user asks for several \
         things at once. Do not use it for one step, or for a question you can simply answer.\n\n\
         Write the list before you start. Mark an item in_progress as you begin it — only one at \
         a time — and completed as soon as it is done, not in a batch at the end. Add steps you \
         discover along the way and remove ones that turn out to be unnecessary. Mark an item \
         completed only when it is actually done: if a test fails or you are blocked, keep it \
         in_progress and add what is needed to unblock it. Write each item as a short \
         imperative, such as \"Add the migration\" or \"Run the tests\". An empty list clears it.",
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "maxItems": MAX_ITEMS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": {
                                "type": "string",
                                "description": "The step, as a short imperative."
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"]
                            }
                        },
                        "required": ["content", "status"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["todos"],
            "additionalProperties": false
        }),
        RiskTier::App,
    )
}

#[derive(Debug, Deserialize)]
struct Args {
    todos: Vec<Todo>,
}

#[derive(Debug, Deserialize)]
struct Todo {
    content: String,
    status: Status,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Status {
    Pending,
    InProgress,
    Completed,
}

#[must_use]
pub fn call(args: &Value) -> ToolOutcome {
    let Args { todos } = match serde_json::from_value(args.clone()) {
        Ok(args) => args,
        Err(err) => {
            return ToolOutcome::error(format!(
                "The list was not understood: {err}. Send {{\"todos\": [{{\"content\": \"…\", \
                 \"status\": \"pending\" | \"in_progress\" | \"completed\"}}]}}."
            ));
        }
    };
    if let Some(problem) = problem(&todos) {
        return ToolOutcome::error(problem);
    }

    let total = todos.len();
    let done = todos
        .iter()
        .filter(|t| t.status == Status::Completed)
        .count();
    let current = todos
        .iter()
        .find(|t| t.status == Status::InProgress)
        .map(|t| t.content.trim());
    let text = if total == 0 {
        "The checklist is cleared.".to_owned()
    } else if done == total {
        format!("Checklist updated: all {total} done.")
    } else {
        match current {
            Some(step) => format!("Checklist updated: {done} of {total} done; now: {step}."),
            None => format!("Checklist updated: {done} of {total} done."),
        }
    };
    ToolOutcome::Complete {
        content: vec![ResultPart::Text { text }],
        structured: Some(json!({ "done": done, "total": total })),
        is_error: false,
        media: Vec::new(),
    }
}

/// What makes a list one the user cannot follow, in a sentence the model can act on.
fn problem(todos: &[Todo]) -> Option<String> {
    if todos.len() > MAX_ITEMS {
        return Some(format!(
            "{} items is more than a checklist can show ({MAX_ITEMS} at most). Group them into \
             steps.",
            todos.len()
        ));
    }
    if let Some(i) = todos.iter().position(|t| t.content.trim().is_empty()) {
        return Some(format!("Item {} is empty. Every item names a step.", i + 1));
    }
    if let Some(t) = todos
        .iter()
        .find(|t| t.content.trim().chars().count() > MAX_CHARS)
    {
        let start: String = t.content.trim().chars().take(40).collect();
        return Some(format!(
            "\"{start}…\" is longer than one line ({MAX_CHARS} characters at most). Name the \
             step; do the detail in the work."
        ));
    }
    let working = todos
        .iter()
        .filter(|t| t.status == Status::InProgress)
        .count();
    if working > 1 {
        return Some(format!(
            "{working} items are in_progress. Mark one — the one you are doing now — and leave \
             the others pending."
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(args: Value) -> (String, Option<Value>, bool) {
        let ToolOutcome::Complete {
            content,
            structured,
            is_error,
            ..
        } = call(&args);
        let text = match &content[0] {
            ResultPart::Text { text } => text.clone(),
            other => panic!("expected text, got {other:?}"),
        };
        (text, structured, is_error)
    }

    fn item(content: &str, status: &str) -> Value {
        json!({ "content": content, "status": status })
    }

    #[test]
    fn a_list_is_counted_and_names_the_step_in_progress() {
        let (text, structured, is_error) = outcome(json!({ "todos": [
            item("Read the failing test", "completed"),
            item("Fix the parser", "in_progress"),
            item("Run the tests", "pending"),
        ]}));
        assert!(!is_error);
        assert_eq!(text, "Checklist updated: 1 of 3 done; now: Fix the parser.");
        assert_eq!(structured, Some(json!({ "done": 1, "total": 3 })));
    }

    #[test]
    fn a_finished_list_and_an_empty_one_say_so() {
        let (done, ..) = outcome(json!({ "todos": [item("Ship it", "completed")] }));
        assert_eq!(done, "Checklist updated: all 1 done.");
        let (cleared, _, is_error) = outcome(json!({ "todos": [] }));
        assert!(!is_error);
        assert_eq!(cleared, "The checklist is cleared.");
    }

    #[test]
    fn only_one_item_is_in_progress_at_a_time() {
        let (text, _, is_error) = outcome(json!({ "todos": [
            item("One", "in_progress"),
            item("Two", "in_progress"),
        ]}));
        assert!(is_error);
        assert!(text.starts_with("2 items are in_progress"), "{text}");
    }

    #[test]
    fn empty_and_overlong_items_are_refused_with_a_reason() {
        let (empty, _, is_error) =
            outcome(json!({ "todos": [item("Fine", "pending"), item("  ", "pending")] }));
        assert!(is_error);
        assert_eq!(empty, "Item 2 is empty. Every item names a step.");

        let (long, _, is_error) =
            outcome(json!({ "todos": [item(&"x".repeat(MAX_CHARS + 1), "pending")] }));
        assert!(is_error);
        assert!(long.contains("longer than one line"), "{long}");

        let many: Vec<Value> = (0..=MAX_ITEMS)
            .map(|i| item(&format!("Step {i}"), "pending"))
            .collect();
        let (text, _, is_error) = outcome(json!({ "todos": many }));
        assert!(is_error);
        assert!(text.contains("more than a checklist can show"), "{text}");
    }

    #[test]
    fn a_status_outside_the_three_is_explained() {
        let (text, _, is_error) = outcome(json!({ "todos": [item("One", "done")] }));
        assert!(is_error);
        assert!(text.starts_with("The list was not understood"), "{text}");
        assert!(text.contains("in_progress"), "{text}");
    }

    #[test]
    fn it_never_asks_and_plan_mode_keeps_it() {
        assert_eq!(definition().tier, RiskTier::App);
    }
}
