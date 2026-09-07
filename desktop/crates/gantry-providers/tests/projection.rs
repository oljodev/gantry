//! One transcript projected onto all four wire formats, asserting the request-side rows of the
//! normalization table (docs/plan/02 §3): system prompt placement, tool definitions and
//! schemas, tool result placement, id round-tripping and sanitizing, parallel calls, tool
//! choice, reasoning replay to its own provider only, server tools, caching, images, notes.

use gantry_core::{
    CallId, ContentPart, MediaSource, Message, MessageId, ProviderKind, ReasoningEffort,
    ResultPart, Role,
};
use gantry_providers::{
    ChatRequest, CompatProfile, ModelCapabilities, ModelInfo, ReasoningSupport, ServerTool,
    ToolSpec, anthropic, gemini, openai_chat, openai_responses,
};
use serde_json::{Value, json};

fn msg(role: Role, parts: Vec<ContentPart>, origin: Option<ProviderKind>) -> Message {
    Message {
        id: MessageId::new(),
        role,
        parts,
        origin,
        created_at: 0,
    }
}

/// A turn made on one provider (`origin`), with thinking from three providers in the assistant
/// message, two parallel calls with ids only that provider would issue, and their results.
fn transcript(origin: ProviderKind) -> Vec<Message> {
    vec![
        msg(
            Role::User,
            vec![
                ContentPart::Text {
                    text: "What time is it?".into(),
                },
                ContentPart::Image {
                    source: MediaSource::Base64 {
                        data: "AAAA".into(),
                    },
                    mime: "image/png".into(),
                },
            ],
            None,
        ),
        msg(
            Role::Assistant,
            vec![
                ContentPart::Thinking {
                    text: "mine".into(),
                    signature: None,
                    provider: ProviderKind::OpenAiChat,
                    item_id: None,
                },
                ContentPart::Thinking {
                    text: "claude".into(),
                    signature: Some("SIG".into()),
                    provider: ProviderKind::Anthropic,
                    item_id: None,
                },
                ContentPart::Thinking {
                    text: "gpt".into(),
                    signature: Some("ENC".into()),
                    provider: ProviderKind::OpenAiResponses,
                    item_id: Some("rs_1".into()),
                },
                ContentPart::Thinking {
                    text: "gem".into(),
                    signature: Some("TS".into()),
                    provider: ProviderKind::Gemini,
                    item_id: None,
                },
                ContentPart::Text {
                    text: "Checking.".into(),
                },
                ContentPart::ToolCall {
                    id: CallId("call:1".into()),
                    name: "gantry__clock".into(),
                    args: json!({ "zone": "local" }),
                    signature: Some("TS_call".into()),
                },
                ContentPart::ToolCall {
                    id: CallId("call:2".into()),
                    name: "gantry__clock".into(),
                    args: json!({}),
                    signature: None,
                },
            ],
            Some(origin),
        ),
        msg(
            Role::Tool,
            vec![
                ContentPart::ToolResult {
                    call_id: CallId("call:1".into()),
                    content: vec![ResultPart::Json {
                        json: json!({ "iso": "2026-09-07T10:00:00+02:00" }),
                    }],
                    is_error: false,
                },
                ContentPart::ToolResult {
                    call_id: CallId("call:2".into()),
                    content: vec![ResultPart::Text { text: "err".into() }],
                    is_error: true,
                },
            ],
            None,
        ),
        msg(
            Role::System,
            vec![ContentPart::SystemNote {
                text: "Be brief.".into(),
            }],
            None,
        ),
        msg(
            Role::User,
            vec![ContentPart::Text {
                text: "Thanks".into(),
            }],
            None,
        ),
    ]
}

fn request(model: &str, origin: ProviderKind) -> ChatRequest {
    let mut req = ChatRequest::new(model, "SYS", transcript(origin));
    req.reasoning = ReasoningEffort::Medium;
    req.max_output_tokens = 4096;
    req.tools = vec![ToolSpec {
        name: "gantry__clock".into(),
        description: "The time".into(),
        input_schema: json!({ "$schema": "x", "type": "object", "properties": { "zone": { "type": "string" } } }),
        strict: false,
        deferred: false,
        stream_args: false,
    }];
    req.server_tools = vec![ServerTool::WebSearch { max_uses: Some(3) }];
    req
}

fn info(reasoning: ReasoningSupport) -> ModelInfo {
    ModelInfo {
        id: "m".into(),
        display_name: "m".into(),
        context_window: None,
        max_output: Some(2048),
        pricing: None,
        capabilities: ModelCapabilities {
            reasoning,
            ..Default::default()
        },
    }
}

fn arr(v: &Value) -> &Vec<Value> {
    v.as_array().expect("array")
}

#[test]
fn anthropic_projection() {
    let req = request("claude-opus-5", ProviderKind::Anthropic);
    let body = anthropic::build_body(&req, Some(&info(ReasoningSupport::Effort)));

    assert_eq!(body["system"][0]["text"], "SYS");
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["max_tokens"], 2048, "capped by the model's max output");
    assert_eq!(body["thinking"]["type"], "adaptive");
    assert_eq!(body["output_config"]["effort"], "medium");
    assert_eq!(body["tool_choice"]["type"], "auto");

    let tools = arr(&body["tools"]);
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["name"], "gantry__clock");
    assert!(tools[0]["input_schema"].get("$schema").is_none());
    assert_eq!(tools[0]["cache_control"]["type"], "ephemeral");
    assert_eq!(tools[1]["type"], "web_search_20260209");
    assert_eq!(tools[1]["max_uses"], 3);

    let messages = arr(&body["messages"]);
    assert_eq!(messages.len(), 5);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"][0]["type"], "text");
    assert_eq!(messages[0]["content"][1]["type"], "image");
    assert_eq!(
        messages[0]["content"][1]["source"]["media_type"],
        "image/png"
    );

    let assistant = arr(&messages[1]["content"]);
    assert_eq!(messages[1]["role"], "assistant");
    let thinking: Vec<&Value> = assistant
        .iter()
        .filter(|b| b["type"] == "thinking")
        .collect();
    assert_eq!(
        thinking.len(),
        1,
        "only Anthropic's own thinking is replayed"
    );
    assert_eq!(thinking[0]["thinking"], "claude");
    assert_eq!(thinking[0]["signature"], "SIG");
    assert_eq!(assistant[1]["type"], "text");
    assert_eq!(assistant[2]["type"], "tool_use");
    assert_eq!(assistant[2]["id"], "call:1", "own ids round-trip unchanged");
    assert_eq!(assistant[2]["input"]["zone"], "local");
    assert_eq!(assistant[3]["id"], "call:2");

    let results = arr(&messages[2]["content"]);
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(
        results.len(),
        2,
        "parallel results travel in one user message"
    );
    assert_eq!(results[0]["type"], "tool_result");
    assert_eq!(results[0]["tool_use_id"], "call:1");
    assert!(results[0]["content"].as_str().unwrap().contains("iso"));
    assert_eq!(results[1]["is_error"], true);

    assert_eq!(messages[3]["role"], "system");
    assert_eq!(messages[3]["content"][0]["text"], "Be brief.");
    assert_eq!(messages[4]["role"], "user");
    assert_eq!(
        messages[4]["content"][0]["cache_control"]["type"], "ephemeral",
        "the last message carries a breakpoint"
    );

    // A turn made elsewhere: ids are sanitized, budgets replace adaptive thinking on 4.5.
    let req = request("claude-sonnet-4-5", ProviderKind::OpenAiChat);
    let body = anthropic::build_body(&req, Some(&info(ReasoningSupport::Budget)));
    assert_eq!(body["thinking"]["type"], "enabled");
    assert_eq!(body["thinking"]["budget_tokens"], 1024);
    assert_eq!(body["messages"][1]["content"][2]["id"], "call_1");
    assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "call_1");
    assert_eq!(body["tools"][1]["type"], "web_search_20250305");

    let mut off = request("claude-opus-5", ProviderKind::Anthropic);
    off.reasoning = ReasoningEffort::Off;
    let body = anthropic::build_body(&off, None);
    assert!(body.get("thinking").is_none());
}

#[test]
fn openai_responses_projection() {
    let req = request("gpt-5", ProviderKind::OpenAiResponses);
    let body = openai_responses::build_body(&req, Some(&info(ReasoningSupport::Effort)));

    assert_eq!(body["instructions"], "SYS");
    assert_eq!(body["store"], false);
    assert_eq!(body["include"][0], "reasoning.encrypted_content");
    assert_eq!(body["reasoning"]["effort"], "medium");
    assert_eq!(body["reasoning"]["summary"], "auto");
    assert_eq!(body["max_output_tokens"], 2048);
    assert_eq!(body["parallel_tool_calls"], true);
    assert_eq!(body["tool_choice"], "auto");

    let tools = arr(&body["tools"]);
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["name"], "gantry__clock");
    assert_eq!(tools[0]["strict"], false);
    assert!(tools[0]["parameters"].get("$schema").is_none());
    assert_eq!(tools[1]["type"], "web_search");

    let input = arr(&body["input"]);
    assert_eq!(input[0]["role"], "user");
    assert_eq!(input[0]["content"][0]["type"], "input_text");
    assert_eq!(input[0]["content"][1]["type"], "input_image");
    assert!(
        input[0]["content"][1]["image_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    let reasoning: Vec<&Value> = input.iter().filter(|i| i["type"] == "reasoning").collect();
    assert_eq!(
        reasoning.len(),
        1,
        "only OpenAI's own reasoning item is replayed"
    );
    assert_eq!(reasoning[0]["id"], "rs_1");
    assert_eq!(reasoning[0]["encrypted_content"], "ENC");
    assert_eq!(reasoning[0]["summary"][0]["text"], "gpt");
    let text = input
        .iter()
        .find(|i| i["role"] == "assistant")
        .expect("assistant text item");
    assert_eq!(text["content"], "Checking.");
    let calls: Vec<&Value> = input
        .iter()
        .filter(|i| i["type"] == "function_call")
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["call_id"], "call:1");
    assert_eq!(calls[0]["arguments"], "{\"zone\":\"local\"}");
    let outputs: Vec<&Value> = input
        .iter()
        .filter(|i| i["type"] == "function_call_output")
        .collect();
    assert_eq!(outputs.len(), 2, "results follow as consecutive items");
    assert_eq!(outputs[0]["call_id"], "call:1");
    assert!(outputs[0]["output"].as_str().unwrap().contains("iso"));
    let dev = input
        .iter()
        .find(|i| i["role"] == "developer")
        .expect("developer note");
    assert_eq!(dev["content"][0]["text"], "Be brief.");
    assert_eq!(input.last().unwrap()["role"], "user");

    let req = request("gpt-5", ProviderKind::Anthropic);
    let body = openai_responses::build_body(&req, None);
    let calls: Vec<&Value> = arr(&body["input"])
        .iter()
        .filter(|i| i["type"] == "function_call")
        .collect();
    assert_eq!(calls[0]["call_id"], "call_1", "foreign ids are sanitized");

    let mut off = request("gpt-5", ProviderKind::OpenAiResponses);
    off.reasoning = ReasoningEffort::Off;
    let body = openai_responses::build_body(&off, Some(&info(ReasoningSupport::Effort)));
    assert!(
        body.get("reasoning").is_none(),
        "off leaves the default alone"
    );
}

#[test]
fn gemini_projection() {
    let req = request("gemini-3-pro", ProviderKind::Gemini);
    let body = gemini::build_body(&req, Some(&info(ReasoningSupport::Effort)));

    assert_eq!(body["system_instruction"], "SYS\n\nBe brief.");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert_eq!(body["generation_config"]["thinking_level"], "medium");
    assert_eq!(body["generation_config"]["tool_choice"], "auto");
    assert_eq!(body["generation_config"]["max_output_tokens"], 2048);

    let tools = arr(&body["tools"]);
    assert_eq!(tools[0]["type"], "function");
    assert!(tools[0]["parameters"].get("$schema").is_none());
    assert_eq!(tools[1]["type"], "google_search");

    let input = arr(&body["input"]);
    assert_eq!(input[0]["role"], "user");
    assert_eq!(input[0]["content"][1]["type"], "image");
    assert_eq!(input[0]["content"][1]["mime_type"], "image/png");
    let model = input
        .iter()
        .find(|i| i["role"] == "model")
        .expect("model turn");
    let kinds: Vec<&str> = arr(&model["content"])
        .iter()
        .map(|c| c["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        ["thought", "text"],
        "only Gemini's own thought is echoed"
    );
    assert_eq!(model["content"][0]["thought_signature"], "TS");
    let calls: Vec<&Value> = input
        .iter()
        .filter(|i| i["type"] == "function_call")
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["id"], "call:1");
    assert_eq!(calls[0]["arguments"]["zone"], "local");
    assert_eq!(calls[0]["thought_signature"], "TS_call");
    assert!(calls[1].get("thought_signature").is_none());
    let results: Vec<&Value> = input
        .iter()
        .filter(|i| i["type"] == "function_result")
        .collect();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["call_id"], "call:1");
    assert_eq!(results[0]["name"], "gantry__clock");
    assert_eq!(results[0]["result"]["iso"], "2026-09-07T10:00:00+02:00");
    assert_eq!(results[1]["result"]["error"], "err");
    assert!(
        !input.iter().any(|i| i["role"] == "system"),
        "notes go into system_instruction"
    );

    let req = request("gemini-3-pro", ProviderKind::Anthropic);
    let body = gemini::build_body(&req, None);
    let calls: Vec<&Value> = arr(&body["input"])
        .iter()
        .filter(|i| i["type"] == "function_call")
        .collect();
    assert_eq!(calls[0]["id"], "call_1");
    assert!(
        calls[0].get("thought_signature").is_none(),
        "foreign signatures are dropped"
    );
    assert!(body["generation_config"].get("thinking_level").is_none());
}

#[test]
fn chat_completions_projection() {
    let req = request("deepseek/deepseek-v4-flash", ProviderKind::OpenAiChat);
    let body = openai_chat::build_body(
        &CompatProfile::openrouter(),
        &req,
        Some(&info(ReasoningSupport::Effort)),
    );
    assert_eq!(body["plugins"][0]["id"], "web");
    assert_eq!(body["plugins"][0]["max_results"], 3);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][2]["reasoning"], "mine");
    assert_eq!(body["messages"][2]["tool_calls"][0]["id"], "call:1");
    assert_eq!(body["messages"][3]["role"], "tool");
    assert_eq!(body["messages"][3]["tool_call_id"], "call:1");
    assert_eq!(body["messages"][5]["role"], "system");
    assert_eq!(body["messages"][5]["content"], "Be brief.");

    let req = request("grok-4", ProviderKind::Gemini);
    let body = openai_chat::build_body(&CompatProfile::xai(), &req, None);
    assert!(body.get("plugins").is_none(), "xAI has no web plugin here");
    assert_eq!(body["messages"][2]["tool_calls"][0]["id"], "call_1");
    assert_eq!(body["messages"][3]["tool_call_id"], "call_1");
}
