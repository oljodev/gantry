//! Projecting a [`ChatRequest`] onto the Interactions API (docs/plan/02 §3, §4, §6): the whole
//! history in `input` (stateless), function calls and results as top-level items, thought
//! signatures echoed where they were captured, system notes appended to `system_instruction`.

use std::collections::HashMap;

use gantry_core::{ContentPart, MediaSource, Message, ProviderKind, ReasoningEffort, Role};
use serde_json::{Value, json};

use crate::{
    provider::{ChatRequest, ModelInfo, ReasoningSupport, ServerTool, ToolChoice},
    tools::{ToolSchemaSanitizer, wire_call_id},
};

fn thinking_level(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Off => "minimal",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High | ReasoningEffort::Max => "high",
    }
}

pub fn build_body(req: &ChatRequest, info: Option<&ModelInfo>) -> Value {
    let max_tokens = match info.and_then(|i| i.max_output) {
        Some(cap) if cap > 0 => req.max_output_tokens.min(cap),
        _ => req.max_output_tokens,
    };
    let thinks = info.is_some_and(|i| i.capabilities.reasoning != ReasoningSupport::None);

    let mut system = req.system.clone();
    for m in req.messages.iter().filter(|m| m.role == Role::System) {
        for p in &m.parts {
            if let Some(text) = p.system_text() {
                if !system.is_empty() {
                    system.push_str("\n\n");
                }
                system.push_str(&text);
            }
        }
    }

    let mut config = json!({ "max_output_tokens": max_tokens });
    if thinks {
        config["thinking_level"] = json!(thinking_level(req.reasoning));
    }
    let mut body = json!({
        "model": req.model,
        "input": input_items(&req.messages),
        "generation_config": config,
        "stream": true,
        "store": false,
    });
    let obj = body.as_object_mut().expect("body is an object");
    if !system.is_empty() {
        obj.insert("system_instruction".into(), json!(system));
    }

    let mut tools: Vec<Value> = Vec::new();
    if !req.tools.is_empty() {
        let sanitizer = ToolSchemaSanitizer::for_provider(ProviderKind::Gemini);
        for t in &req.tools {
            tools.push(json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "parameters": sanitizer.sanitize(&t.input_schema, false),
            }));
        }
        let choice = match &req.tool_choice {
            ToolChoice::Auto => "auto",
            ToolChoice::None => "none",
            ToolChoice::Required | ToolChoice::Named(_) => "any",
        };
        obj["generation_config"]["tool_choice"] = json!(choice);
    }
    for tool in &req.server_tools {
        match tool {
            ServerTool::WebSearch { .. } => tools.push(json!({ "type": "google_search" })),
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
    // `function_result` items name the function; the call that asked is looked up by id.
    let mut names: HashMap<String, (String, Option<ProviderKind>)> = HashMap::new();
    let mut items = Vec::new();
    for m in transcript {
        match m.role {
            Role::User => {
                let content = user_content(&m.parts);
                if !content.is_empty() {
                    items.push(json!({ "role": "user", "content": content }));
                }
            }
            Role::System => {}
            Role::Assistant => {
                let mut content: Vec<Value> = Vec::new();
                let flush = |content: &mut Vec<Value>, items: &mut Vec<Value>| {
                    if !content.is_empty() {
                        items.push(json!({ "role": "model", "content": std::mem::take(content) }));
                    }
                };
                for p in &m.parts {
                    match p {
                        ContentPart::Text { text } if !text.trim().is_empty() => {
                            content.push(json!({ "type": "text", "text": text }));
                        }
                        ContentPart::Thinking {
                            signature: Some(sig),
                            provider: ProviderKind::Gemini,
                            ..
                        } => {
                            content.push(json!({ "type": "thought", "thought_signature": sig }));
                        }
                        ContentPart::ToolCall {
                            id,
                            name,
                            args,
                            signature,
                        } => {
                            flush(&mut content, &mut items);
                            let wire_id = wire_call_id(id.as_str(), m.origin, ProviderKind::Gemini);
                            names.insert(id.as_str().to_owned(), (name.clone(), m.origin));
                            let mut item = json!({
                                "type": "function_call",
                                "id": wire_id,
                                "name": name,
                                "arguments": if args.is_object() { args.clone() } else { json!({}) },
                            });
                            if let (Some(sig), Some(ProviderKind::Gemini)) = (signature, m.origin) {
                                item["thought_signature"] = json!(sig);
                            }
                            items.push(item);
                        }
                        _ => {}
                    }
                }
                flush(&mut content, &mut items);
            }
            Role::Tool => {
                for p in &m.parts {
                    if let ContentPart::ToolResult {
                        call_id: id,
                        content,
                        is_error,
                    } = p
                    {
                        let (name, origin) = names
                            .get(id.as_str())
                            .cloned()
                            .unwrap_or_else(|| ("unknown".to_owned(), None));
                        let result = match content.as_slice() {
                            [gantry_core::ResultPart::Json { json }] if json.is_object() => {
                                json.clone()
                            }
                            _ => {
                                let text = crate::anthropic::result_text(content);
                                if *is_error {
                                    json!({ "error": text })
                                } else {
                                    json!({ "output": text })
                                }
                            }
                        };
                        items.push(json!({
                            "type": "function_result",
                            "call_id": wire_call_id(id.as_str(), origin, ProviderKind::Gemini),
                            "name": name,
                            "result": result,
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
            ContentPart::Text { text } | ContentPart::TurnContext { text, .. }
                if !text.is_empty() =>
            {
                Some(json!({ "type": "text", "text": text }))
            }
            ContentPart::Image {
                source: MediaSource::Base64 { data },
                mime,
            } => Some(json!({ "type": "image", "mime_type": mime, "data": data })),
            ContentPart::Document {
                source: MediaSource::Base64 { data },
                mime,
                ..
            } => Some(json!({ "type": "document", "mime_type": mime, "data": data })),
            _ => None,
        })
        .collect()
}
