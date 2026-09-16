//! Anthropic's append-only rule, checked three steps deep (docs/plan/02 §1, §6; 09 M13).
//!
//! Claude binds a thinking block to the exact prefix — `system`, `tools`, the messages before it
//! — that produced it, and a request whose prefix has drifted is refused. Everything Gantry does
//! about a chat that changes underneath the model follows from that: the system prompt is frozen
//! when the chat is made, an instruction change becomes a `system` **message** rather than an
//! edit, and a tool set that had to be rebuilt says so in the request instead of hoping.
//!
//! This is the offline half of the check: the same chat, three turns, each one changing
//! something that a naïve implementation would have rewritten the prefix for, and the bytes
//! compared. The other half is the live run of 02 §8, which sends `prefix_mismatch_behavior:
//! "error"` so that Anthropic — the only party that knows what it bound a block to — is the one
//! that answers.

use gantry_core::{ContentPart, Message, MessageId, ProviderKind, ReasoningEffort, Role};
use gantry_providers::{ChatRequest, PREFIX_DROP_BLOCK, PREFIX_ERROR, ToolSpec, anthropic};
use serde_json::Value;

const MODEL: &str = "claude-opus-5";

/// The frozen prompt, written once when the chat is created and never again (10 §2).
const SYSTEM: &str = "<gantry_core version=\"8\">You are Gantry.</gantry_core>";

fn tool(name: &str) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: "does a thing".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        strict: false,
        deferred: false,
        stream_args: false,
    }
}

fn msg(role: Role, parts: Vec<ContentPart>) -> Message {
    Message {
        id: MessageId::new(),
        role,
        parts,
        origin: Some(ProviderKind::Anthropic),
        created_at: 0,
    }
}

fn user(text: &str) -> Message {
    msg(Role::User, vec![ContentPart::Text { text: text.into() }])
}

/// An assistant turn with thinking in it — the thing the prefix binding is about.
fn assistant(text: &str) -> Message {
    msg(
        Role::Assistant,
        vec![
            ContentPart::Thinking {
                text: "let me see".into(),
                signature: Some("sig".into()),
                provider: ProviderKind::Anthropic,
                item_id: None,
            },
            ContentPart::Text { text: text.into() },
        ],
    )
}

fn request(messages: Vec<Message>, tools: Vec<ToolSpec>) -> ChatRequest {
    let mut req = ChatRequest::new(MODEL, SYSTEM, messages);
    req.tools = tools;
    req.reasoning = ReasoningEffort::Medium;
    req
}

fn body(req: &ChatRequest) -> Value {
    anthropic::build_body(req, None)
}

/// The two halves of the prefix, as the bytes they go on the wire as.
fn prefix(body: &Value) -> (String, String) {
    (
        serde_json::to_string(&body["system"]).unwrap(),
        serde_json::to_string(&body["tools"]).unwrap(),
    )
}

#[test]
fn three_turns_of_one_chat_keep_the_same_prefix() {
    let tools = vec![tool("fake__echo")];

    // Turn one: an ordinary question.
    let first = body(&request(vec![user("what time is it?")], tools.clone()));

    // Turn two: between the turns the user edited their global instructions *and* changed a
    // memory the chat was holding (10 §4, 12 §B6). A chat that has spoken keeps the prompt it
    // was answering and is *told* what changed, so both arrive as messages and the frozen text
    // is untouched. They are the same `SystemNote` part, which is the point: one path, so
    // neither can grow a way of rewriting the prefix that the other does not have.
    let second = body(&request(
        vec![
            user("what time is it?"),
            assistant("just past four"),
            msg(
                Role::System,
                vec![ContentPart::SystemNote {
                    text: "The user's instructions changed: answer in Norwegian.".into(),
                }],
            ),
            msg(
                Role::System,
                vec![ContentPart::SystemNote {
                    text: "Forget what you were told about the user's timezone.".into(),
                }],
            ),
            user("and now?"),
        ],
        tools.clone(),
    ));

    assert_eq!(
        prefix(&first),
        prefix(&second),
        "an instruction change must not rewrite the prefix"
    );
    let notes: String = second["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "system")
        .map(|m| serde_json::to_string(m).unwrap())
        .collect();
    assert!(notes.contains("Norwegian"), "the instruction note: {notes}");
    assert!(notes.contains("timezone"), "the memory note: {notes}");

    // Turn three: a connector was attached while the chat was open, so the tool array is not
    // the one the earlier thinking blocks were bound to. The system half still may not move;
    // the tool half must, and the request has to say what to do about it.
    let mut grown = tools.clone();
    grown.push(tool("github__search"));
    let third = body(&request(
        vec![
            user("what time is it?"),
            assistant("just past four"),
            user("and now?"),
            assistant("kvart over fire"),
            msg(
                Role::System,
                vec![ContentPart::ToolSetChange {
                    added: vec!["github".into()],
                    removed: Vec::new(),
                }],
            ),
            user("find the issue"),
        ],
        grown,
    ));

    assert_eq!(
        prefix(&first).0,
        prefix(&third).0,
        "the frozen prompt is frozen for the life of the chat"
    );
    assert_ne!(
        prefix(&first).1,
        prefix(&third).1,
        "the tool array really was rebuilt"
    );
    assert_eq!(
        third["thinking"]["block_binding"]["prefix_mismatch_behavior"], PREFIX_DROP_BLOCK,
        "and the request says so: {}",
        third["thinking"]
    );
}

/// The forgiveness is asked for only when there is something to forgive. A request that carries
/// it every time would hide a prefix that drifted for a reason nobody intended.
#[test]
fn a_chat_whose_tools_never_changed_asks_for_nothing() {
    let first = body(&request(
        vec![user("hello"), assistant("hi"), user("again")],
        vec![tool("fake__echo")],
    ));
    assert!(
        first["thinking"]["block_binding"].is_null(),
        "{}",
        first["thinking"]
    );
}

/// A tool declared with `defer_loading` is turned on with a `tool_addition` block, which leaves
/// the array alone — so a change that only involves deferred tools is not a rebuild. Nothing
/// declares one yet; this is what stops the check from starting to lie on the day something does.
#[test]
fn a_deferred_tool_arriving_is_not_a_rebuilt_prefix() {
    let mut deferred = tool("github__search");
    deferred.deferred = true;
    let req = request(
        vec![
            user("hello"),
            assistant("hi"),
            msg(
                Role::System,
                vec![ContentPart::ToolSetChange {
                    added: vec!["github".into()],
                    removed: Vec::new(),
                }],
            ),
            user("find the issue"),
        ],
        vec![tool("fake__echo"), deferred],
    );
    let body = body(&req);
    assert!(
        body["thinking"]["block_binding"].is_null(),
        "{}",
        body["thinking"]
    );
    let github = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "github__search")
        .unwrap();
    assert_eq!(github["defer_loading"], true);
}

/// Detaching a connector is a rebuild too: the array the blocks were bound to is gone whether
/// the change added to it or took from it.
#[test]
fn a_connector_detached_mid_chat_is_also_a_rebuild() {
    let req = request(
        vec![
            user("hello"),
            assistant("hi"),
            msg(
                Role::System,
                vec![ContentPart::ToolSetChange {
                    added: Vec::new(),
                    removed: vec!["github".into()],
                }],
            ),
            user("never mind"),
        ],
        vec![tool("fake__echo")],
    );
    assert_eq!(
        body(&req)["thinking"]["block_binding"]["prefix_mismatch_behavior"],
        PREFIX_DROP_BLOCK
    );
}

/// What the live run sends instead (02 §8): the same request, asking to be refused rather than
/// forgiven, so that a prefix Gantry believes is stable is checked by the only party that knows.
#[test]
fn the_live_run_can_ask_to_be_refused_instead() {
    let mut req = request(
        vec![
            user("hello"),
            assistant("hi"),
            msg(
                Role::System,
                vec![ContentPart::ToolSetChange {
                    added: vec!["github".into()],
                    removed: Vec::new(),
                }],
            ),
            user("find it"),
        ],
        vec![tool("fake__echo"), tool("github__search")],
    );
    req.prefix_mismatch = PREFIX_ERROR;
    assert_eq!(
        body(&req)["thinking"]["block_binding"]["prefix_mismatch_behavior"],
        PREFIX_ERROR
    );
}

/// Thinking off, nothing bound, nothing to say. The field lives inside `thinking`, and a request
/// that sends no `thinking` must not grow one.
#[test]
fn a_chat_without_thinking_carries_no_binding_at_all() {
    let mut req = request(
        vec![
            user("hello"),
            msg(
                Role::System,
                vec![ContentPart::ToolSetChange {
                    added: vec!["github".into()],
                    removed: Vec::new(),
                }],
            ),
            user("find it"),
        ],
        vec![tool("github__search")],
    );
    req.reasoning = ReasoningEffort::Off;
    let body = body(&req);
    assert!(body["thinking"].is_null(), "{body}");
}
