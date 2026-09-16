//! The live conformance run (docs/plan/02 §8): one ignored test per provider, each running
//! the scenarios below against a real key and printing a summary line per scenario, including
//! whether partial tool arguments streamed (13 §2 depends on that column).
//!
//! Keys come from the environment only; nothing is read from the app's vault:
//!
//! ```text
//! OPENROUTER_API_KEY=… cargo test -p gantry-providers --test live openrouter -- --ignored --nocapture
//! ANTHROPIC_API_KEY=…  cargo test -p gantry-providers --test live anthropic  -- --ignored --nocapture
//! OPENAI_API_KEY=…     cargo test -p gantry-providers --test live openai     -- --ignored --nocapture
//! GEMINI_API_KEY=…     cargo test -p gantry-providers --test live gemini     -- --ignored --nocapture
//! XAI_API_KEY=…        cargo test -p gantry-providers --test live xai        -- --ignored --nocapture
//! ```
//!
//! Each run costs a few cents on a small model. `LIVE_MODEL` overrides the model,
//! `LIVE_WEB_SEARCH=1` adds the web search scenario (it costs more).

use std::sync::Arc;

use futures_util::StreamExt;
use gantry_core::{
    CallId, ContentPart, MediaSource, Message, MessageId, ProviderId, ReasoningEffort, ResultPart,
    Role, StopReason,
};
use gantry_providers::{
    AnthropicProvider, ChatRequest, CompatProfile, GeminiProvider, OpenAiChatProvider,
    OpenAiResponsesProvider, Provider, ServerTool, StreamEvent, ToolSpec, http_client,
};
use gantry_secrets::SecretString;

const CLOCK: &str = "gantry__clock";

fn key(name: &str) -> Option<SecretString> {
    std::env::var(name).ok().map(SecretString::from)
}

fn model(default: &str) -> String {
    std::env::var("LIVE_MODEL").unwrap_or_else(|_| default.to_owned())
}

fn clock_tool() -> ToolSpec {
    ToolSpec {
        name: CLOCK.into(),
        description: "The current date and time on the user's computer. Call it once per zone."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": { "zone": { "type": "string", "enum": ["local", "utc"], "description": "Which clock" } },
            "required": ["zone"]
        }),
        strict: false,
        deferred: false,
        stream_args: false,
    }
}

#[derive(Debug, Default)]
struct Collected {
    text: String,
    thinking: String,
    parts: Vec<ContentPart>,
    calls: Vec<(u32, CallId, String, serde_json::Value)>,
    arg_deltas: usize,
    stop: Option<StopReason>,
    usage: Option<gantry_core::Usage>,
}

/// Runs one request to the end (or until `stop_after_first_text`) and folds the events.
async fn run(
    provider: &Arc<dyn Provider>,
    req: ChatRequest,
    stop_after_first_text: bool,
) -> Result<Collected, gantry_providers::ProviderError> {
    let mut stream = provider.stream(req).await?;
    let mut c = Collected::default();
    let mut names: std::collections::BTreeMap<u32, (CallId, String)> = Default::default();
    let mut thinking_sig: std::collections::BTreeMap<u32, String> = Default::default();
    while let Some(ev) = stream.next().await {
        match ev? {
            StreamEvent::TextDelta { text, .. } => {
                c.text.push_str(&text);
                if stop_after_first_text {
                    return Ok(c);
                }
            }
            StreamEvent::ThinkingDelta { text, .. } => c.thinking.push_str(&text),
            StreamEvent::ThinkingSignature { index, signature } => {
                thinking_sig.insert(index, signature);
            }
            StreamEvent::ToolCallStart { index, id, name } => {
                names.insert(index, (id, name));
            }
            StreamEvent::ToolCallArgsDelta { .. } => c.arg_deltas += 1,
            StreamEvent::ToolCallEnd { index, args } => {
                if let Some((id, name)) = names.get(&index) {
                    c.calls
                        .push((index, id.clone(), name.clone(), args.clone()));
                    c.parts.push(ContentPart::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        args,
                        signature: None,
                    });
                }
            }
            StreamEvent::ProviderBlock { index, part } => {
                // A signed call re-issued whole replaces the unsigned one.
                if let ContentPart::ToolCall { id, .. } = &part
                    && let Some(pos) = c
                        .parts
                        .iter()
                        .position(|p| matches!(p, ContentPart::ToolCall { id: i, .. } if i == id))
                {
                    c.parts[pos] = part;
                } else {
                    let _ = index;
                    c.parts.push(part);
                }
            }
            StreamEvent::Usage(u) => c.usage = Some(u),
            StreamEvent::MessageEnd { stop_reason } => c.stop = Some(stop_reason),
            StreamEvent::MessageStart { .. } | StreamEvent::Notice { .. } => {}
        }
    }
    if !c.thinking.is_empty() {
        c.parts.insert(
            0,
            ContentPart::Thinking {
                text: c.thinking.clone(),
                signature: thinking_sig.into_values().next(),
                provider: provider.kind(),
                item_id: None,
            },
        );
    }
    if !c.text.is_empty() {
        c.parts.push(ContentPart::Text {
            text: c.text.clone(),
        });
    }
    Ok(c)
}

fn line(name: &str, ok: bool, detail: impl std::fmt::Display) {
    eprintln!("  [{}] {name}: {detail}", if ok { "ok" } else { "FAIL" });
}

/// The scenario list of 02 §8 minus the ones that need a later milestone (tool-set change)
/// or cannot be forced (refusal).
async fn conformance(provider: Arc<dyn Provider>, model: &str) {
    eprintln!("== {} · {model}", provider.id());
    let mut failures = 0;

    // 1. key and model list
    match provider.check_key().await {
        Ok(info) => line("key", true, format!("{info:?}")),
        Err(e) => {
            line("key", false, e);
            failures += 1;
        }
    }
    match provider.list_models().await {
        Ok(models) => line("models", true, format!("{} models", models.len())),
        Err(e) => {
            line("models", false, e);
            failures += 1;
        }
    }

    // 2. text
    let mut req = ChatRequest::new(
        model,
        "Answer in one short sentence.",
        vec![Message::user_text("What is a gantry crane?")],
    );
    req.max_output_tokens = 200;
    req.reasoning = ReasoningEffort::Low;
    match run(&provider, req, false).await {
        Ok(c) => {
            let ok = !c.text.is_empty() && c.stop == Some(StopReason::EndTurn);
            line(
                "text",
                ok,
                format!(
                    "{:?} · {} chars · usage {:?}",
                    c.stop,
                    c.text.len(),
                    c.usage
                ),
            );
            failures += usize::from(!ok);
        }
        Err(e) => {
            line("text", false, e);
            failures += 1;
        }
    }

    // 3. parallel tools with streamed arguments, then 4. a tool error and thinking replay
    let mut req = ChatRequest::new(
        model,
        "You have a clock tool. When asked for the time in two zones, call the tool twice in \
         the same reply, once with zone local and once with zone utc.",
        vec![Message::user_text(
            "What time is it in my local zone and in UTC? Use the tool for both.",
        )],
    );
    req.max_output_tokens = 600;
    req.reasoning = ReasoningEffort::Low;
    req.tools = vec![clock_tool()];
    let first = run(&provider, req.clone(), false).await;
    let mut streamed = "n/a".to_owned();
    match &first {
        Ok(c) => {
            let ok = !c.calls.is_empty() && c.stop == Some(StopReason::ToolUse);
            streamed = if c.arg_deltas > 0 {
                format!("yes ({} deltas)", c.arg_deltas)
            } else {
                "no (arguments arrived whole)".to_owned()
            };
            line(
                "tools",
                ok,
                format!(
                    "{} calls · {:?} · streams partial arguments: {streamed}",
                    c.calls.len(),
                    c.stop
                ),
            );
            failures += usize::from(!ok);
        }
        Err(e) => {
            line("tools", false, e);
            failures += 1;
        }
    }
    if let Ok(c) = first
        && !c.calls.is_empty()
    {
        let assistant = Message {
            id: MessageId::new(),
            role: Role::Assistant,
            parts: c.parts.clone(),
            origin: Some(provider.kind()),
            created_at: 0,
        };
        let results: Vec<ContentPart> = c
            .calls
            .iter()
            .enumerate()
            .map(|(i, (_, id, _, _))| ContentPart::ToolResult {
                call_id: id.clone(),
                content: vec![if i == 0 {
                    ResultPart::Json {
                        json: serde_json::json!({ "iso": "2026-09-07T10:00:00+02:00", "zone": "local" }),
                    }
                } else {
                    ResultPart::Text {
                        text: "clock unavailable: permission denied".into(),
                    }
                }],
                is_error: i != 0,
            })
            .collect();
        let tool = Message {
            id: MessageId::new(),
            role: Role::Tool,
            parts: results,
            origin: None,
            created_at: 0,
        };
        let mut round2 = req.clone();
        round2.messages.push(assistant);
        round2.messages.push(tool);
        match run(&provider, round2, false).await {
            Ok(c) => {
                let ok = !c.text.is_empty();
                line(
                    "tool round 2 (thinking replayed, one error result)",
                    ok,
                    format!(
                        "{:?} · {}",
                        c.stop,
                        c.text.chars().take(120).collect::<String>()
                    ),
                );
                failures += usize::from(!ok);
            }
            Err(e) => {
                line("tool round 2", false, e);
                failures += 1;
            }
        }
    }

    // 5. max tokens
    let mut req = ChatRequest::new(
        model,
        "",
        vec![Message::user_text("Write three paragraphs about cranes.")],
    );
    req.max_output_tokens = 40;
    req.reasoning = ReasoningEffort::Off;
    match run(&provider, req, false).await {
        Ok(c) => {
            let ok = c.stop == Some(StopReason::MaxTokens);
            line("max tokens", ok, format!("{:?}", c.stop));
            failures += usize::from(!ok);
        }
        Err(e) => {
            line("max tokens", false, e);
            failures += 1;
        }
    }

    // 6. cancel mid-stream: drop the stream after the first text delta
    let mut req = ChatRequest::new(model, "", vec![Message::user_text("Count from 1 to 200.")]);
    req.max_output_tokens = 400;
    req.reasoning = ReasoningEffort::Off;
    match run(&provider, req, true).await {
        Ok(c) => line(
            "cancel mid-stream",
            true,
            format!("dropped after {:?}", c.text),
        ),
        Err(e) => {
            line("cancel mid-stream", false, e);
            failures += 1;
        }
    }

    // 7. image input: a 1×1 red PNG
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    let mut req = ChatRequest::new(
        model,
        "",
        vec![Message {
            id: MessageId::new(),
            role: Role::User,
            parts: vec![
                ContentPart::Text {
                    text: "What colour is this image? One word.".into(),
                },
                ContentPart::Image {
                    source: MediaSource::Base64 { data: png.into() },
                    mime: "image/png".into(),
                },
            ],
            origin: None,
            created_at: 0,
        }],
    );
    req.max_output_tokens = 50;
    req.reasoning = ReasoningEffort::Off;
    match run(&provider, req, false).await {
        Ok(c) => line("image input", true, c.text.trim()),
        Err(e) => line(
            "image input",
            false,
            format!("{e} (fine for text-only models)"),
        ),
    }

    // 8. cache hit on turn two: a long stable prefix sent twice
    let long_system = format!(
        "You are Gantry. Reference notes follow.\n{}",
        "The quick brown fox jumps over the lazy dog. ".repeat(220)
    );
    let mut req = ChatRequest::new(model, long_system, vec![Message::user_text("Say hi.")]);
    req.max_output_tokens = 20;
    req.reasoning = ReasoningEffort::Off;
    let _ = run(&provider, req.clone(), false).await;
    match run(&provider, req, false).await {
        // A verification, not a report: the same prefix sent twice and no cache read means the
        // prefix moved between the two, which is the failure this scenario exists to catch
        // (02 §3, 09 M13). A provider with no prompt cache at all reports nothing and says so.
        Ok(c) => {
            let read = c.usage.map_or(0, |u| u.cache_read);
            let wrote = c.usage.map_or(0, |u| u.cache_write);
            line(
                "cache hit on turn two",
                read > 0,
                format!("cache_read {read} · cache_write {wrote}"),
            );
            failures += usize::from(read == 0);
        }
        Err(e) => {
            line("cache hit on turn two", false, e);
            failures += 1;
        }
    }

    // 9. web search (opt-in: it costs)
    if std::env::var("LIVE_WEB_SEARCH").is_ok() {
        let mut req = ChatRequest::new(
            model,
            "",
            vec![Message::user_text(
                "Search the web: what is today's top headline on the BBC? One sentence.",
            )],
        );
        req.max_output_tokens = 300;
        req.reasoning = ReasoningEffort::Off;
        req.server_tools = vec![ServerTool::WebSearch { max_uses: Some(2) }];
        match run(&provider, req, false).await {
            Ok(c) => line(
                "web search",
                true,
                format!(
                    "{} opaque blocks · {}",
                    c.parts
                        .iter()
                        .filter(|p| matches!(p, ContentPart::ProviderOpaque { .. }))
                        .count(),
                    c.text.chars().take(100).collect::<String>()
                ),
            ),
            Err(e) => line("web search", false, e),
        }
    }

    eprintln!(
        "== {} done · {failures} failures · streams partial tool arguments: {streamed}",
        provider.id()
    );
    assert_eq!(failures, 0, "see the lines above");
}

#[tokio::test]
#[ignore = "needs OPENROUTER_API_KEY and spends a few cents"]
async fn openrouter() {
    let Some(key) = key("OPENROUTER_API_KEY") else {
        eprintln!("OPENROUTER_API_KEY not set");
        return;
    };
    let p: Arc<dyn Provider> = Arc::new(OpenAiChatProvider::new(
        ProviderId::openrouter(),
        CompatProfile::openrouter(),
        Some(key),
        http_client("test"),
        Vec::new(),
    ));
    let _ = p.list_models().await;
    conformance(p, &model("deepseek/deepseek-v4-flash")).await;
}

#[tokio::test]
#[ignore = "needs ANTHROPIC_API_KEY and spends a few cents"]
async fn anthropic() {
    let Some(key) = key("ANTHROPIC_API_KEY") else {
        eprintln!("ANTHROPIC_API_KEY not set");
        return;
    };
    let p: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        ProviderId::new("anthropic"),
        None,
        Some(key),
        http_client("test"),
        Vec::new(),
    ));
    let _ = p.list_models().await;
    conformance(p, &model("claude-haiku-4-5")).await;
}

#[tokio::test]
#[ignore = "needs OPENAI_API_KEY and spends a few cents"]
async fn openai() {
    let Some(key) = key("OPENAI_API_KEY") else {
        eprintln!("OPENAI_API_KEY not set");
        return;
    };
    let p: Arc<dyn Provider> = Arc::new(OpenAiResponsesProvider::new(
        ProviderId::new("openai"),
        None,
        Some(key),
        http_client("test"),
        Vec::new(),
    ));
    let _ = p.list_models().await;
    conformance(p, &model("gpt-5-mini")).await;
}

#[tokio::test]
#[ignore = "needs GEMINI_API_KEY and spends a few cents"]
async fn gemini() {
    let Some(key) = key("GEMINI_API_KEY") else {
        eprintln!("GEMINI_API_KEY not set");
        return;
    };
    let p: Arc<dyn Provider> = Arc::new(GeminiProvider::new(
        ProviderId::new("google"),
        None,
        Some(key),
        http_client("test"),
        Vec::new(),
    ));
    let _ = p.list_models().await;
    conformance(p, &model("gemini-2.5-flash")).await;
}

#[tokio::test]
#[ignore = "needs XAI_API_KEY and spends a few cents"]
async fn xai() {
    let Some(key) = key("XAI_API_KEY") else {
        eprintln!("XAI_API_KEY not set");
        return;
    };
    let p: Arc<dyn Provider> = Arc::new(OpenAiChatProvider::new(
        ProviderId::new("xai"),
        CompatProfile::xai(),
        Some(key),
        http_client("test"),
        Vec::new(),
    ));
    let _ = p.list_models().await;
    conformance(p, &model("grok-4-fast-non-reasoning")).await;
}
