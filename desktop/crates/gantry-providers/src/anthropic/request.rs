//! Projecting a [`ChatRequest`] onto the Messages API (docs/plan/02 §3, §4, §6).
//!
//! The prefix stays byte-identical across turns by construction: frozen system text, the tool
//! list in the caller's (sorted) order, no timestamps. Cache breakpoints sit after the system
//! prompt, after the tool list and on the last message.

use gantry_core::{ContentPart, MediaSource, Message, ProviderKind, ReasoningEffort, Role};
use serde_json::{Value, json};

use std::collections::HashMap;

use crate::{
    provider::{ChatRequest, ModelInfo, ReasoningSupport, ServerTool, ToolChoice},
    tools::{ToolSchemaSanitizer, call_origins, wire_call_id},
};

/// `(major, minor)` of a Claude model id: `claude-opus-4-6` → (4, 6), `claude-3-5-haiku…` →
/// (3, 5), `claude-opus-5` → (5, 0), `claude-sonnet-4-20250514` → (4, 0).
#[must_use]
pub fn version(model: &str) -> Option<(u32, u32)> {
    let mut parts = model.split('-').filter(|p| !p.is_empty());
    let major = parts.find_map(|p| (p.len() <= 2).then(|| p.parse::<u32>().ok()).flatten())?;
    let minor = parts
        .next()
        .filter(|p| p.len() <= 2)
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(0);
    Some((major, minor))
}

/// Whether the model takes adaptive thinking with an effort level (4.6 and later) rather than
/// a token budget.
#[must_use]
pub fn adaptive_thinking(model: &str) -> bool {
    matches!(version(model), Some((major, minor)) if major >= 5 || (major == 4 && minor >= 6))
}

/// The web search tool type the model accepts.
#[must_use]
pub fn web_search_type(model: &str) -> &'static str {
    if adaptive_thinking(model) {
        "web_search_20260209"
    } else {
        "web_search_20250305"
    }
}

fn effort_name(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Off | ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Max => "max",
    }
}

/// A thinking budget for models that take one, always below `max_tokens`.
fn budget_for(effort: ReasoningEffort, max_tokens: u32) -> u32 {
    let wanted = match effort {
        ReasoningEffort::Off => 0,
        ReasoningEffort::Low => 2_048,
        ReasoningEffort::Medium => 8_192,
        ReasoningEffort::High => 24_576,
        ReasoningEffort::Max => 32_000,
    };
    wanted.min(max_tokens.saturating_sub(1_024)).max(1_024)
}

pub fn build_body(req: &ChatRequest, info: Option<&ModelInfo>) -> Value {
    let max_tokens = match info.and_then(|i| i.max_output) {
        Some(cap) if cap > 0 => req.max_output_tokens.min(cap),
        _ => req.max_output_tokens,
    };
    let reasoning = info.map_or(
        if adaptive_thinking(&req.model) {
            ReasoningSupport::Effort
        } else {
            ReasoningSupport::Budget
        },
        |i| i.capabilities.reasoning,
    );

    let mut body = json!({
        "model": req.model,
        "max_tokens": max_tokens,
        "stream": true,
        "messages": messages(&req.messages),
    });
    let obj = body.as_object_mut().expect("body is an object");

    if !req.system.is_empty() {
        obj.insert(
            "system".into(),
            json!([{ "type": "text", "text": req.system, "cache_control": { "type": "ephemeral" } }]),
        );
    }

    match (reasoning, req.reasoning) {
        (ReasoningSupport::None, _) | (_, ReasoningEffort::Off) => {}
        (ReasoningSupport::Effort, effort) => {
            obj.insert("thinking".into(), json!({ "type": "adaptive" }));
            obj.insert(
                "output_config".into(),
                json!({ "effort": effort_name(effort) }),
            );
        }
        (ReasoningSupport::Budget, effort) => {
            obj.insert(
                "thinking".into(),
                json!({ "type": "enabled", "budget_tokens": budget_for(effort, max_tokens) }),
            );
        }
    }

    let mut tools: Vec<Value> = Vec::new();
    if !req.tools.is_empty() {
        let sanitizer = ToolSchemaSanitizer::for_provider(ProviderKind::Anthropic);
        for t in &req.tools {
            let mut tool = json!({
                "name": t.name,
                "description": t.description,
                "input_schema": sanitizer.sanitize(&t.input_schema, t.strict),
            });
            if t.strict {
                tool["strict"] = json!(true);
            }
            if t.deferred {
                tool["defer_loading"] = json!(true);
            }
            if t.stream_args {
                tool["eager_input_streaming"] = json!(true);
            }
            tools.push(tool);
        }
        if let Some(last) = tools.last_mut() {
            last["cache_control"] = json!({ "type": "ephemeral" });
        }
        // `any` and `tool` are refused by current models once thinking is on; the instruction
        // in the prompt carries the intent instead (02 §3).
        let choice = match &req.tool_choice {
            ToolChoice::None => json!({ "type": "none" }),
            ToolChoice::Auto | ToolChoice::Required | ToolChoice::Named(_) => {
                json!({ "type": "auto" })
            }
        };
        obj.insert("tool_choice".into(), choice);
    }
    for tool in &req.server_tools {
        match tool {
            ServerTool::WebSearch { max_uses } => {
                let mut t = json!({ "type": web_search_type(&req.model), "name": "web_search" });
                if let Some(n) = max_uses {
                    t["max_uses"] = json!(n);
                }
                tools.push(t);
            }
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

/// The transcript as wire messages: consecutive same-role messages merge, tool results ride
/// in user messages, and the last message's last block carries the cache breakpoint.
fn messages(transcript: &[Message]) -> Vec<Value> {
    let origins = call_origins(transcript);
    let mut out: Vec<(&'static str, Vec<Value>)> = Vec::new();
    let mut push = |role: &'static str, blocks: Vec<Value>| {
        if blocks.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((r, existing)) if *r == role && role != "system" => existing.extend(blocks),
            _ => out.push((role, blocks)),
        }
    };
    for m in transcript {
        match m.role {
            Role::User => push("user", user_blocks(&m.parts)),
            Role::Assistant => push("assistant", assistant_blocks(m)),
            Role::Tool => push("user", tool_result_blocks(m, &origins)),
            Role::System => push("system", system_blocks(&m.parts)),
        }
    }
    if let Some((_, blocks)) = out.last_mut()
        && let Some(last) = blocks.last_mut()
        && last.get("type").and_then(Value::as_str) != Some("thinking")
    {
        last["cache_control"] = json!({ "type": "ephemeral" });
    }
    out.into_iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect()
}

fn user_blocks(parts: &[ContentPart]) -> Vec<Value> {
    parts
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text { text } if !text.is_empty() => {
                Some(json!({ "type": "text", "text": text }))
            }
            ContentPart::Image {
                source: MediaSource::Base64 { data },
                mime,
            } => Some(json!({
                "type": "image",
                "source": { "type": "base64", "media_type": mime, "data": data }
            })),
            ContentPart::Document {
                source: MediaSource::Base64 { data },
                mime,
                name,
            } => Some(json!({
                "type": "document",
                "source": { "type": "base64", "media_type": mime, "data": data },
                "title": name
            })),
            _ => None,
        })
        .collect()
}

fn system_blocks(parts: &[ContentPart]) -> Vec<Value> {
    parts
        .iter()
        .filter_map(|p| {
            p.system_text()
                .map(|text| json!({ "type": "text", "text": text }))
        })
        .collect()
}

fn assistant_blocks(m: &Message) -> Vec<Value> {
    m.parts
        .iter()
        .filter_map(|p| match p {
            ContentPart::Thinking {
                text,
                signature: Some(sig),
                provider: ProviderKind::Anthropic,
                ..
            } => Some(json!({ "type": "thinking", "thinking": text, "signature": sig })),
            ContentPart::ProviderOpaque {
                provider: ProviderKind::Anthropic,
                json,
                ..
            } => Some(json.clone()),
            ContentPart::Text { text } if !text.trim().is_empty() => {
                Some(json!({ "type": "text", "text": text }))
            }
            ContentPart::ToolCall { id, name, args, .. } => Some(json!({
                "type": "tool_use",
                "id": wire_call_id(id.as_str(), m.origin, ProviderKind::Anthropic),
                "name": name,
                "input": if args.is_object() { args.clone() } else { json!({}) },
            })),
            _ => None,
        })
        .collect()
}

fn tool_result_blocks(m: &Message, origins: &HashMap<String, ProviderKind>) -> Vec<Value> {
    m.parts
        .iter()
        .filter_map(|p| match p {
            ContentPart::ToolResult {
                call_id: id,
                content,
                is_error,
            } => {
                let has_image = content
                    .iter()
                    .any(|r| matches!(r, gantry_core::ResultPart::Image { .. }));
                let body = if has_image {
                    Value::Array(
                        content
                            .iter()
                            .map(|r| match r {
                                gantry_core::ResultPart::Image { data, mime } => json!({
                                    "type": "image",
                                    "source": { "type": "base64", "media_type": mime, "data": data }
                                }),
                                other => json!({ "type": "text", "text": result_text(std::slice::from_ref(other)) }),
                            })
                            .collect(),
                    )
                } else {
                    json!(result_text(content))
                };
                let mut block = json!({
                    "type": "tool_result",
                    "tool_use_id": wire_call_id(
                        id.as_str(),
                        origins.get(id.as_str()).copied(),
                        ProviderKind::Anthropic
                    ),
                    "content": body,
                });
                if *is_error {
                    block["is_error"] = json!(true);
                }
                Some(block)
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn result_text(content: &[gantry_core::ResultPart]) -> String {
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
    use super::*;

    #[test]
    fn versions_parse_from_every_id_shape() {
        assert_eq!(version("claude-opus-4-6"), Some((4, 6)));
        assert_eq!(version("claude-sonnet-4-5-20250929"), Some((4, 5)));
        assert_eq!(version("claude-3-5-haiku-20241022"), Some((3, 5)));
        assert_eq!(version("claude-opus-5"), Some((5, 0)));
        assert_eq!(version("claude-fable-5-1"), Some((5, 1)));
        assert_eq!(version("claude-sonnet-4-20250514"), Some((4, 0)));
        assert!(adaptive_thinking("claude-opus-4-6"));
        assert!(adaptive_thinking("claude-sonnet-5"));
        assert!(!adaptive_thinking("claude-sonnet-4-5"));
        assert_eq!(web_search_type("claude-haiku-4-5"), "web_search_20250305");
        assert_eq!(web_search_type("claude-opus-5"), "web_search_20260209");
    }

    #[test]
    fn budgets_stay_below_max_tokens() {
        assert_eq!(budget_for(ReasoningEffort::High, 4096), 3072);
        assert_eq!(budget_for(ReasoningEffort::Low, 64000), 2048);
        assert_eq!(budget_for(ReasoningEffort::Low, 1500), 1024);
    }
}
