//! Projecting a [`ChatRequest`] onto the Chat Completions wire format (docs/plan/02 §3, §6).

use gantry_core::{ContentPart, Message, ProviderKind, ReasoningEffort, Role};
use serde_json::{Value, json};

use super::profiles::{CompatProfile, ReasoningParam};
use crate::provider::{ChatRequest, ModelInfo, ReasoningSupport, ToolChoice};

/// `info` is what the model list said about the model, when it is known: the reasoning
/// parameter is sent only to models that accept it, and `max_tokens` never exceeds the
/// model's maximum output. An unknown model gets the conservative body.
pub fn build_body(profile: &CompatProfile, req: &ChatRequest, info: Option<&ModelInfo>) -> Value {
    let supports_reasoning =
        info.is_some_and(|i| i.capabilities.reasoning != ReasoningSupport::None);
    let max_tokens = match info.and_then(|i| i.max_output) {
        Some(cap) if cap > 0 => req.max_output_tokens.min(cap),
        _ => req.max_output_tokens,
    };
    let mut messages = Vec::with_capacity(req.messages.len() + 1);
    if !req.system.is_empty() {
        messages.push(json!({ "role": profile.system_role, "content": req.system }));
    }
    for m in &req.messages {
        messages.extend(project(profile, m));
    }

    let mut body = json!({
        "model": req.model,
        "messages": messages,
        "stream": true,
        "max_tokens": max_tokens,
    });
    let obj = body.as_object_mut().expect("body is an object");

    if profile.supports_stream_usage {
        obj.insert("stream_options".into(), json!({ "include_usage": true }));
    }
    match (profile.reasoning_param, req.reasoning) {
        _ if !supports_reasoning => {}
        (_, ReasoningEffort::Off) | (ReasoningParam::None, _) => {}
        (ReasoningParam::OpenRouterObject, effort) => {
            obj.insert("reasoning".into(), json!({ "effort": effort_name(effort) }));
        }
        (ReasoningParam::OpenAiEffort, effort) => {
            obj.insert("reasoning_effort".into(), json!(effort_name(effort)));
        }
    }
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                let mut f = json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                });
                if profile.supports_strict && t.strict {
                    f["strict"] = json!(true);
                }
                json!({ "type": "function", "function": f })
            })
            .collect();
        obj.insert("tools".into(), Value::Array(tools));
        let choice = match &req.tool_choice {
            ToolChoice::Auto => json!("auto"),
            ToolChoice::None => json!("none"),
            ToolChoice::Required => json!("required"),
            ToolChoice::Named(name) => json!({ "type": "function", "function": { "name": name } }),
        };
        obj.insert("tool_choice".into(), choice);
        if profile.supports_parallel_flag {
            obj.insert("parallel_tool_calls".into(), json!(true));
        }
    }
    if let Some(extra) = req.provider_options.as_object() {
        for (k, v) in extra {
            obj.insert(k.clone(), v.clone());
        }
    }
    body
}

fn effort_name(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Off => "none",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Max => "max",
    }
}

/// One transcript message → zero or more wire messages.
fn project(profile: &CompatProfile, m: &Message) -> Vec<Value> {
    match m.role {
        Role::User => vec![json!({ "role": "user", "content": user_content(&m.parts) })],
        Role::System => {
            let text: Vec<&str> = m
                .parts
                .iter()
                .filter_map(|p| match p {
                    ContentPart::SystemNote { text } => Some(text.as_str()),
                    ContentPart::ToolSetChange { added, removed } => {
                        let _ = (added, removed);
                        None
                    }
                    _ => None,
                })
                .collect();
            if text.is_empty() {
                Vec::new()
            } else {
                vec![json!({ "role": profile.system_role, "content": text.join("\n\n") })]
            }
        }
        Role::Assistant => {
            let mut text = String::new();
            let mut reasoning = String::new();
            let mut tool_calls = Vec::new();
            for p in &m.parts {
                match p {
                    ContentPart::Text { text: t } => text.push_str(t),
                    ContentPart::Thinking {
                        text: t,
                        provider: ProviderKind::OpenAiChat,
                        ..
                    } => reasoning.push_str(t),
                    ContentPart::ToolCall { id, name, args } => tool_calls.push(json!({
                        "id": id.as_str(),
                        "type": "function",
                        "function": { "name": name, "arguments": args.to_string() },
                    })),
                    _ => {}
                }
            }
            let mut msg = json!({ "role": "assistant", "content": text });
            if !reasoning.is_empty() {
                msg["reasoning"] = json!(reasoning);
            }
            if !tool_calls.is_empty() {
                msg["tool_calls"] = Value::Array(tool_calls);
            }
            vec![msg]
        }
        Role::Tool => m
            .parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::ToolResult {
                    call_id, content, ..
                } => Some(json!({
                    "role": "tool",
                    "tool_call_id": call_id.as_str(),
                    "content": result_text(content),
                })),
                _ => None,
            })
            .collect(),
    }
}

fn user_content(parts: &[ContentPart]) -> Value {
    let has_media = parts.iter().any(|p| matches!(p, ContentPart::Image { .. }));
    if !has_media {
        let text: String = parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        return json!(text);
    }
    let items: Vec<Value> = parts
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text { text } => Some(json!({ "type": "text", "text": text })),
            ContentPart::Image {
                source: gantry_core::MediaSource::Base64 { data },
                mime,
            } => Some(json!({
                "type": "image_url",
                "image_url": { "url": format!("data:{mime};base64,{data}") }
            })),
            _ => None,
        })
        .collect();
    Value::Array(items)
}

fn result_text(content: &[gantry_core::ResultPart]) -> String {
    content
        .iter()
        .map(|r| match r {
            gantry_core::ResultPart::Text { text } => text.clone(),
            gantry_core::ResultPart::Json { json } => json.to_string(),
            gantry_core::ResultPart::Image { .. } => "[image]".to_owned(),
            gantry_core::ResultPart::Resource { summary, .. } => summary.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use gantry_core::MessageId;

    use super::*;
    use crate::provider::ModelCapabilities;

    fn info(reasoning: ReasoningSupport, max_output: Option<u32>) -> ModelInfo {
        ModelInfo {
            id: "m".into(),
            display_name: "m".into(),
            context_window: None,
            max_output,
            pricing: None,
            capabilities: ModelCapabilities {
                reasoning,
                ..Default::default()
            },
        }
    }

    #[test]
    fn reasoning_is_sent_only_to_models_that_accept_it_and_max_tokens_is_capped() {
        let mut req = ChatRequest::new("google/gemma-3-27b-it", "", vec![Message::user_text("hi")]);
        req.reasoning = ReasoningEffort::Medium;
        req.max_output_tokens = 8192;
        let plain = info(ReasoningSupport::None, Some(4096));
        let body = build_body(&CompatProfile::openrouter(), &req, Some(&plain));
        assert!(
            body.get("reasoning").is_none(),
            "no reasoning for a plain model"
        );
        assert_eq!(body["max_tokens"], 4096);
        let unknown = build_body(&CompatProfile::openrouter(), &req, None);
        assert!(
            unknown.get("reasoning").is_none(),
            "unknown models get the conservative body"
        );
        assert_eq!(unknown["max_tokens"], 8192);
    }

    #[test]
    fn openrouter_body_has_system_first_reasoning_object_and_no_usage_flag() {
        let mut req = ChatRequest::new(
            "deepseek/deepseek-v4-flash",
            "You are Gantry.",
            vec![Message::user_text("hi")],
        );
        req.reasoning = ReasoningEffort::Low;
        req.max_output_tokens = 512;
        let thinking = info(ReasoningSupport::Effort, None);
        let body = build_body(&CompatProfile::openrouter(), &req, Some(&thinking));
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "hi");
        assert_eq!(body["reasoning"]["effort"], "low");
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(body["stream"], true);
        assert!(body.get("stream_options").is_none());
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn assistant_thinking_is_replayed_only_for_the_same_provider() {
        let assistant = Message {
            id: MessageId::new(),
            role: Role::Assistant,
            parts: vec![
                ContentPart::Thinking {
                    text: "mine".into(),
                    signature: None,
                    provider: ProviderKind::OpenAiChat,
                },
                ContentPart::Thinking {
                    text: "theirs".into(),
                    signature: None,
                    provider: ProviderKind::Anthropic,
                },
                ContentPart::Text {
                    text: "answer".into(),
                },
            ],
            origin: Some(ProviderKind::OpenAiChat),
            created_at: 0,
        };
        let req = ChatRequest::new("m", "", vec![assistant]);
        let body = build_body(&CompatProfile::openrouter(), &req, None);
        assert_eq!(body["messages"][0]["reasoning"], "mine");
        assert_eq!(body["messages"][0]["content"], "answer");
    }
}
