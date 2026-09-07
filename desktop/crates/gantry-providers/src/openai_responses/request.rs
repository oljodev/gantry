//! Projecting a [`ChatRequest`] onto the Responses API (docs/plan/02 §3, §4, §6): the system
//! prompt as `instructions`, one input item per part, reasoning items replayed with their
//! encrypted content, function results as `function_call_output` items.

use gantry_core::{ContentPart, MediaSource, Message, ProviderKind, ReasoningEffort, Role};
use serde_json::{Value, json};

use crate::{
    provider::{ChatRequest, ModelInfo, ReasoningSupport, ServerTool, ToolChoice},
    tools::{ToolSchemaSanitizer, call_origins, wire_call_id},
};

fn effort_name(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Off | ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High | ReasoningEffort::Max => "high",
    }
}

pub fn build_body(req: &ChatRequest, info: Option<&ModelInfo>) -> Value {
    let max_tokens = match info.and_then(|i| i.max_output) {
        Some(cap) if cap > 0 => req.max_output_tokens.min(cap),
        _ => req.max_output_tokens,
    };
    // Unknown models are treated as reasoning models: every current OpenAI chat model is.
    let reasons = info.is_none_or(|i| i.capabilities.reasoning != ReasoningSupport::None);

    let mut body = json!({
        "model": req.model,
        "input": input_items(&req.messages),
        "stream": true,
        "store": false,
        "max_output_tokens": max_tokens,
        "parallel_tool_calls": true,
    });
    let obj = body.as_object_mut().expect("body is an object");
    if !req.system.is_empty() {
        obj.insert("instructions".into(), json!(req.system));
    }
    if reasons {
        obj.insert("include".into(), json!(["reasoning.encrypted_content"]));
        // "Off" leaves the model's default in place: the accepted lowest level varies by
        // family (`minimal` on gpt-5, `none` on gpt-5.1+), and a wrong one is a 400.
        if req.reasoning != ReasoningEffort::Off {
            obj.insert(
                "reasoning".into(),
                json!({ "effort": effort_name(req.reasoning), "summary": "auto" }),
            );
        }
    }

    let mut tools: Vec<Value> = Vec::new();
    if !req.tools.is_empty() {
        let sanitizer = ToolSchemaSanitizer::for_provider(ProviderKind::OpenAiResponses);
        for t in &req.tools {
            tools.push(json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "parameters": sanitizer.sanitize(&t.input_schema, t.strict),
                "strict": t.strict,
            }));
        }
        let choice = match &req.tool_choice {
            ToolChoice::Auto => json!("auto"),
            ToolChoice::None => json!("none"),
            ToolChoice::Required => json!("required"),
            ToolChoice::Named(name) => json!({ "type": "function", "name": name }),
        };
        obj.insert("tool_choice".into(), choice);
    }
    for tool in &req.server_tools {
        match tool {
            ServerTool::WebSearch { .. } => tools.push(json!({ "type": "web_search" })),
        }
    }
    if !tools.is_empty() {
        obj.insert("tools".into(), Value::Array(tools));
    }
    if let Some(extra) = req.provider_options.as_object() {
        for (k, v) in extra {
            obj.insert(k.clone(), v.clone());
        }
    }
    body
}

fn input_items(transcript: &[Message]) -> Vec<Value> {
    let origins = call_origins(transcript);
    let mut items = Vec::new();
    for m in transcript {
        match m.role {
            Role::User => {
                let content = user_content(&m.parts);
                if !content.is_empty() {
                    items.push(json!({ "type": "message", "role": "user", "content": content }));
                }
            }
            Role::System => {
                let text: Vec<&str> = m
                    .parts
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::SystemNote { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                if !text.is_empty() {
                    items.push(json!({
                        "type": "message",
                        "role": "developer",
                        "content": [{ "type": "input_text", "text": text.join("\n\n") }]
                    }));
                }
            }
            Role::Assistant => {
                for p in &m.parts {
                    match p {
                        ContentPart::Thinking {
                            text,
                            signature: Some(encrypted),
                            provider: ProviderKind::OpenAiResponses,
                            item_id: Some(id),
                        } => {
                            let summary = if text.trim().is_empty() {
                                json!([])
                            } else {
                                json!([{ "type": "summary_text", "text": text }])
                            };
                            items.push(json!({
                                "type": "reasoning",
                                "id": id,
                                "summary": summary,
                                "encrypted_content": encrypted,
                            }));
                        }
                        ContentPart::Text { text } if !text.trim().is_empty() => {
                            items.push(json!({ "role": "assistant", "content": text }));
                        }
                        ContentPart::ToolCall { id, name, args, .. } => items.push(json!({
                            "type": "function_call",
                            "call_id": wire_call_id(id.as_str(), m.origin, ProviderKind::OpenAiResponses),
                            "name": name,
                            "arguments": args.to_string(),
                        })),
                        ContentPart::ProviderOpaque {
                            provider: ProviderKind::OpenAiResponses,
                            json,
                            ..
                        } => items.push(json.clone()),
                        _ => {}
                    }
                }
            }
            Role::Tool => {
                for p in &m.parts {
                    if let ContentPart::ToolResult {
                        call_id: id,
                        content,
                        ..
                    } = p
                    {
                        items.push(json!({
                            "type": "function_call_output",
                            "call_id": wire_call_id(
                                id.as_str(),
                                origins.get(id.as_str()).copied(),
                                ProviderKind::OpenAiResponses
                            ),
                            "output": crate::anthropic::result_text(content),
                        }));
                    }
                }
            }
        }
    }
    items
}

fn user_content(parts: &[ContentPart]) -> Vec<Value> {
    parts
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text { text } if !text.is_empty() => {
                Some(json!({ "type": "input_text", "text": text }))
            }
            ContentPart::Image {
                source: MediaSource::Base64 { data },
                mime,
            } => Some(json!({
                "type": "input_image",
                "image_url": format!("data:{mime};base64,{data}"),
            })),
            ContentPart::Document {
                source: MediaSource::Base64 { data },
                mime,
                name,
            } => Some(json!({
                "type": "input_file",
                "filename": name,
                "file_data": format!("data:{mime};base64,{data}"),
            })),
            _ => None,
        })
        .collect()
}
