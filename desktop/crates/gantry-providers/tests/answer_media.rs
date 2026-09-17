//! What each wire format does with a picture a tool put in the answer (docs/plan/03 §4).
//!
//! `ToolOutcome::media` lands in the transcript as an assistant message holding nothing but
//! media, so the reply can render it where the call happened. None of it is meant to go back to
//! a chat model: the model was shown whatever the tool's own result carried, and no provider
//! accepts a picture from the assistant side anyway. So the rule for all four clients is the
//! same — the message projects to **nothing at all**, not to an empty assistant turn between
//! two real ones.

use gantry_core::{
    CallId, ContentPart, MediaSource, Message, MessageId, ReasoningEffort, ResultPart, Role,
};
use gantry_providers::{
    ChatRequest, CompatProfile, anthropic, gemini, openai_chat, openai_responses,
};
use serde_json::Value;

fn msg(role: Role, parts: Vec<ContentPart>) -> Message {
    Message {
        id: MessageId::new(),
        role,
        parts,
        origin: None,
        created_at: 0,
    }
}

/// A turn that called a tool, got a picture from it, and carried on: exactly the shape the
/// `media` connector produces.
fn transcript() -> Vec<Message> {
    vec![
        msg(
            Role::User,
            vec![ContentPart::Text {
                text: "draw a bicycle".into(),
            }],
        ),
        msg(
            Role::Assistant,
            vec![
                ContentPart::Text {
                    text: "One bicycle:".into(),
                },
                ContentPart::ToolCall {
                    id: CallId::from("call_1".to_owned()),
                    name: "media__generate".into(),
                    args: serde_json::json!({ "prompt": "a bicycle" }),
                    signature: None,
                },
            ],
        ),
        msg(
            Role::Tool,
            vec![ContentPart::ToolResult {
                call_id: CallId::from("call_1".to_owned()),
                content: vec![ResultPart::Text {
                    text: "Made a picture.".into(),
                }],
                is_error: false,
            }],
        ),
        // The one this file is about.
        msg(
            Role::Assistant,
            vec![ContentPart::Image {
                source: MediaSource::Base64 {
                    data: "AAAA".into(),
                },
                mime: "image/png".into(),
            }],
        ),
        msg(
            Role::Assistant,
            vec![ContentPart::Text {
                text: "There it is.".into(),
            }],
        ),
    ]
}

fn request() -> ChatRequest {
    let mut req = ChatRequest::new("a-model", "You are Gantry.", transcript());
    req.reasoning = ReasoningEffort::Off;
    req
}

/// Every string in the body, so "the bytes are nowhere in the request" can be asked once.
fn text_of(body: &Value) -> String {
    serde_json::to_string(body).expect("a request serialises")
}

#[test]
fn anthropic_sends_no_message_for_it() {
    let body = anthropic::build_body(&request(), None);
    let messages = body["messages"].as_array().expect("messages");
    let roles: Vec<&str> = messages
        .iter()
        .map(|m| m["role"].as_str().unwrap_or_default())
        .collect();
    // user, assistant (text + tool_use), user (the tool result), assistant ("There it is.")
    assert_eq!(roles, ["user", "assistant", "user", "assistant"]);
    assert!(
        !text_of(&body).contains("AAAA"),
        "the picture is not replayed to the model"
    );
}

#[test]
fn chat_completions_sends_no_message_for_it() {
    let body = openai_chat::build_body(&CompatProfile::openrouter(), &request(), None);
    let messages = body["messages"].as_array().expect("messages");
    let roles: Vec<&str> = messages
        .iter()
        .map(|m| m["role"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(roles, ["system", "user", "assistant", "tool", "assistant"]);
    assert!(
        !messages
            .iter()
            .any(|m| m["role"] == "assistant" && m["content"] == ""),
        "an assistant turn with nothing in it is worse than no turn: {messages:?}"
    );
    assert!(!text_of(&body).contains("AAAA"));
}

#[test]
fn responses_sends_no_item_for_it() {
    let body = openai_responses::build_body(&request(), None);
    assert!(!text_of(&body).contains("AAAA"));
    let items = body["input"].as_array().expect("input items");
    assert!(
        items.iter().all(|i| i["type"] != "input_image"),
        "the picture reached the input items: {items:?}"
    );
}

#[test]
fn gemini_sends_no_part_for_it() {
    let body = gemini::build_body(&request(), None);
    assert!(!text_of(&body).contains("AAAA"));
}
