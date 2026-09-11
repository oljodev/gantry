//! Context management (docs/plan/02 §6): keeping a long chat inside the model's window without
//! ever editing what was said.
//!
//! Three rules shape everything here.
//!
//! **The transcript is append-only.** Nothing is deleted, rewritten or trimmed in place. A
//! compaction appends one `System` message carrying [`ContentPart::Compacted`], and that marker
//! says which messages it stands for. The rows it covers stay in the database and stay on the
//! screen; only the request skips them ([`live`]). This is not tidiness — current Claude models
//! bind thinking blocks to the exact prefix that produced them, and an edited prefix is a 400
//! (02 §1), so a client that rewrites history is a client that breaks on its own transcript.
//!
//! **Compaction happens between turns, never inside one.** A turn's own tool calls and their
//! results have to reach the model together, and summarizing a conversation the assistant is
//! still in the middle of is how a tool loop loses the thread. So the check runs when a turn
//! finishes, sizing the request the *next* turn will make. What defends a single runaway turn is
//! the tool-result cap and the round limit, not this.
//!
//! **The estimate uses real numbers where they exist.** After a turn, the provider has just told
//! us what the last round's prompt cost and what it answered with; their sum is very close to
//! what the next request's prefix will be. That beats counting characters, which is the fallback
//! for a provider that reported nothing.

use std::sync::Arc;

use futures_util::StreamExt;
use gantry_core::{ContentPart, Message, MessageId, ProviderKind, ReasoningEffort, Role, Usage};
use gantry_providers::{ChatRequest, Provider, ProviderError, StreamEvent};

const SUMMARIZER: &str = include_str!("../../../assets/prompts/compaction.md");

/// The share of the window at which the next request is considered too close to the edge.
pub const COMPACT_AT: f64 = 0.75;
/// What a model whose window nobody knows is assumed to have. Low enough to be safe, high
/// enough not to compact a chat that was never in trouble.
pub const DEFAULT_WINDOW: u32 = 128_000;
/// Turns kept verbatim by keep-tail compaction.
pub const KEEP_TURNS: usize = 3;
/// Fewer messages than this is not worth a model call.
const MIN_SPAN: usize = 4;
/// A rough count for an image, which no character count describes.
const IMAGE_TOKENS: usize = 1_500;
/// Framing per message: role, separators, the provider's own wrapper.
const PER_MESSAGE_TOKENS: usize = 8;
const CHARS_PER_TOKEN: usize = 4;
/// What the summarizer is given of the span, at most.
const MAX_SPAN_CHARS: usize = 80_000;
/// What one tool result contributes to that rendering.
const RESULT_CHARS: usize = 600;

/// The messages a request actually carries: the newest compaction marker first, then everything
/// the marker does not cover. The marker is appended at the end of the turn that made it, so
/// this is also what puts it back in front, where a summary of the beginning belongs.
#[must_use]
pub fn live(messages: &[Message]) -> Vec<Message> {
    let found = messages.iter().enumerate().rev().find_map(|(i, m)| {
        m.parts.iter().find_map(|p| match p {
            ContentPart::Compacted { up_to, .. } => Some((i, *up_to)),
            _ => None,
        })
    });
    let Some((marker_at, up_to)) = found else {
        return messages.to_vec();
    };
    // A marker whose span cannot be found is a marker we do not trust: keep everything rather
    // than guess, and let the budget check compact again.
    let Some(cut) = messages.iter().position(|m| m.id == up_to) else {
        return messages.to_vec();
    };
    let mut out = vec![messages[marker_at].clone()];
    out.extend(
        messages
            .iter()
            .enumerate()
            .filter(|(i, _)| *i > cut && *i != marker_at)
            .map(|(_, m)| m.clone()),
    );
    out
}

/// What the next request will cost, in tokens, as well as it can be known.
///
/// `last` is the final round's usage: its input is the prompt the provider just charged for and
/// its output is the answer that has since been appended, so the two together are the next
/// prefix. That number already contains the system prompt and the tool schemas, which is why
/// `overhead` — their counted size — is added only on the other path. Without usage, characters
/// are counted instead, good to maybe ±25%, which is why the threshold is three quarters of the
/// window rather than the edge of it.
#[must_use]
pub fn estimate(messages: &[Message], overhead: u32, last: Option<&Usage>) -> u32 {
    if let Some(usage) = last
        && usage.input > 0
    {
        let measured = usage.input + usage.output + usage.cache_read;
        return u32::try_from(measured).unwrap_or(u32::MAX);
    }
    counted(messages).saturating_add(overhead)
}

/// The part of every request that is not the transcript: the frozen system prompt and the tool
/// schemas. On a chat with a dozen connectors attached this is thousands of tokens, and leaving
/// it out is how a budget check decides there is room when there is not.
#[must_use]
pub fn overhead(system: &str, tools: &[gantry_providers::ToolSpec]) -> u32 {
    let tools: usize = tools
        .iter()
        .map(|t| t.name.len() + t.description.len() + t.input_schema.to_string().len())
        .sum();
    text_tokens(system).saturating_add(u32::try_from(tools / CHARS_PER_TOKEN).unwrap_or(u32::MAX))
}

/// The character-counting estimate, used when the provider reported no usage at all.
#[must_use]
pub fn counted(messages: &[Message]) -> u32 {
    let total: usize = messages
        .iter()
        .map(|m| PER_MESSAGE_TOKENS + m.parts.iter().map(part_tokens).sum::<usize>())
        .sum();
    u32::try_from(total).unwrap_or(u32::MAX)
}

fn part_tokens(part: &ContentPart) -> usize {
    let chars = match part {
        ContentPart::Text { text } | ContentPart::Thinking { text, .. } => text.len(),
        ContentPart::SystemNote { text } => text.len(),
        ContentPart::Compacted { summary, .. } => summary.len(),
        ContentPart::ToolCall { name, args, .. } => name.len() + args.to_string().len(),
        ContentPart::ToolResult { content, .. } => {
            gantry_core::result_preview(content, usize::MAX).len()
        }
        ContentPart::ProviderOpaque { json, .. } => json.to_string().len(),
        ContentPart::ToolSetChange { added, removed } => {
            added.iter().chain(removed).map(String::len).sum::<usize>()
        }
        ContentPart::Image { .. } => return IMAGE_TOKENS,
        // Sound and video are replaced by a sentence on the way to a provider (02 §5).
        ContentPart::Audio { .. } | ContentPart::Video { .. } | ContentPart::Document { .. } => 20,
    };
    chars / CHARS_PER_TOKEN
}

/// Tokens a piece of prose costs: the system prompt and the tool schemas, which are part of
/// every request and are not small.
#[must_use]
pub fn text_tokens(text: &str) -> u32 {
    u32::try_from(text.len() / CHARS_PER_TOKEN).unwrap_or(u32::MAX)
}

/// Whether a request of this size is too close to the window to send another one after it.
#[must_use]
pub fn over_budget(estimate: u32, window: Option<u32>) -> bool {
    let window = window.filter(|w| *w > 0).unwrap_or(DEFAULT_WINDOW);
    f64::from(estimate) > f64::from(window) * COMPACT_AT
}

/// How many turns a provider's compaction may keep verbatim.
///
/// Anthropic keeps none. Keep-tail leaves older thinking blocks in a prefix that no longer
/// matches what produced them, so 02 §6 forbids it there: the summary replaces the whole
/// history and the transcript continues, append-only, from the marker. Every other provider
/// gets the last few turns as they were said, which is worth a great deal on a task that has
/// been going for a while.
#[must_use]
pub fn keep_turns(kind: ProviderKind) -> usize {
    match kind {
        ProviderKind::Anthropic => 0,
        _ => KEEP_TURNS,
    }
}

/// The messages to summarize: everything up to the point where `keep` turns remain. `None` when
/// there is not enough history for the summary to buy anything.
///
/// The cut is always at a turn boundary — a user message — so a tool call never ends up in the
/// summary while its result stays in the transcript, or the other way round.
#[must_use]
pub fn span(messages: &[Message], keep: usize) -> Option<std::ops::Range<usize>> {
    let starts: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::User)
        .map(|(i, _)| i)
        .collect();
    let cut = if keep == 0 {
        messages.len()
    } else if starts.len() > keep {
        starts[starts.len() - keep]
    } else {
        return None;
    };
    (cut >= MIN_SPAN).then_some(0..cut)
}

/// The artifacts made in a span (13 §7). They outlive the messages that made them, so the
/// marker names them and the model can read one back instead of rebuilding it.
#[must_use]
pub fn artifacts_in(span: &[Message]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for message in span {
        for part in &message.parts {
            let ContentPart::ToolCall { name, args, .. } = part else {
                continue;
            };
            if !name.ends_with("create_artifact") && !name.ends_with("update_artifact") {
                continue;
            }
            if let Some(id) = args.get("artifact_id").and_then(|v| v.as_str()) {
                let id = id.to_owned();
                if !out.contains(&id) {
                    out.push(id);
                }
            } else if let Some(title) = args.get("title").and_then(|v| v.as_str()) {
                let title = title.to_owned();
                if !out.contains(&title) {
                    out.push(title);
                }
            }
        }
    }
    out
}

/// The span as the summarizer reads it: one line per thing that happened, results trimmed to
/// what identifies them. A summarizer given the raw transcript would spend its window on the
/// tool output the compaction exists to get rid of.
#[must_use]
pub fn render(span: &[Message]) -> String {
    let mut out = String::new();
    for message in span {
        for part in &message.parts {
            match part {
                ContentPart::Text { text } if !text.trim().is_empty() => {
                    let who = match message.role {
                        Role::User => "User",
                        Role::Assistant => "Assistant",
                        _ => "Note",
                    };
                    out.push_str(&format!("{who}: {}\n", text.trim()));
                }
                ContentPart::ToolCall { name, args, .. } => {
                    out.push_str(&format!(
                        "Assistant called {name} {}\n",
                        clip(&args.to_string(), 300)
                    ));
                }
                ContentPart::ToolResult {
                    content, is_error, ..
                } => {
                    let text = gantry_core::result_preview(content, usize::MAX);
                    out.push_str(&format!(
                        "  {} {}\n",
                        if *is_error { "failed:" } else { "result:" },
                        clip(text.trim(), RESULT_CHARS)
                    ));
                }
                other => {
                    if let Some(text) = other.system_text() {
                        out.push_str(&format!("Note: {}\n", text.trim()));
                    }
                }
            }
        }
    }
    clip_middle(&out, MAX_SPAN_CHARS)
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut s: String = text.chars().take(max).collect();
    s.push('…');
    s
}

/// Keeps both ends: the start of a conversation holds the goal and the end holds where it got to.
fn clip_middle(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_owned();
    }
    let half = max / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = text.chars().skip(count - half).collect::<String>();
    format!(
        "{head}\n\n[… {} characters of the middle omitted …]\n\n{tail}",
        count - 2 * half
    )
}

/// The summary itself: one short request to the cheapest fast model of the same provider, the
/// one the title generator and the judge use.
pub async fn summarize(
    provider: Arc<dyn Provider>,
    model: String,
    span: &[Message],
    artifacts: &[String],
) -> Result<String, ProviderError> {
    let mut prompt = render(span);
    if !artifacts.is_empty() {
        prompt.push_str(&format!(
            "\n\nArtifacts made in this part of the conversation: {}.\n",
            artifacts.join(", ")
        ));
    }
    prompt.push_str("\n\nSummarize the conversation above.");
    let mut req = ChatRequest::new(model, SUMMARIZER, vec![Message::user_text(prompt)]);
    req.max_output_tokens = 2_000;
    req.reasoning = ReasoningEffort::Off;
    let mut stream = provider.stream(req).await?;
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event? {
            StreamEvent::TextDelta { text: t, .. } => text.push_str(&t),
            StreamEvent::MessageEnd { .. } => break,
            _ => {}
        }
    }
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err(ProviderError::new(
            gantry_core::ProviderErrorKind::InvalidRequest,
            "the summarizer returned nothing",
        ));
    }
    Ok(text)
}

/// The marker message a compaction appends.
#[must_use]
pub fn marker(
    summary: String,
    up_to: MessageId,
    replaced: usize,
    artifacts: Vec<String>,
) -> Message {
    Message {
        id: MessageId::new(),
        role: Role::System,
        parts: vec![ContentPart::Compacted {
            summary,
            up_to,
            replaced: u32::try_from(replaced).unwrap_or(u32::MAX),
            artifacts,
        }],
        origin: None,
        created_at: gantry_core::now_ms(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gantry_core::{CallId, ResultPart};

    fn user(text: &str) -> Message {
        Message::user_text(text)
    }

    fn assistant(text: &str) -> Message {
        Message {
            id: MessageId::new(),
            role: Role::Assistant,
            parts: vec![ContentPart::Text { text: text.into() }],
            origin: Some(ProviderKind::OpenAiChat),
            created_at: 0,
        }
    }

    fn chat(turns: usize) -> Vec<Message> {
        (0..turns)
            .flat_map(|i| [user(&format!("q{i}")), assistant(&format!("a{i}"))])
            .collect()
    }

    #[test]
    fn a_chat_with_no_marker_is_its_own_live_transcript() {
        let messages = chat(3);
        assert_eq!(live(&messages), messages);
    }

    #[test]
    fn the_marker_comes_first_however_late_it_was_appended() {
        let mut messages = chat(4);
        let up_to = messages[3].id;
        // Appended at the end, which is where an append-only transcript puts it.
        messages.push(marker("what happened".into(), up_to, 4, Vec::new()));

        let live = live(&messages);
        assert!(
            matches!(live[0].parts[0], ContentPart::Compacted { .. }),
            "a summary of the beginning belongs at the beginning"
        );
        assert_eq!(live.len(), 5, "the marker plus the four messages it left");
        assert_eq!(live[1].text(), "q2");
        assert!(
            live.iter().all(|m| m.text() != "q0"),
            "the summarized messages are not sent again"
        );
    }

    #[test]
    fn a_marker_pointing_at_nothing_leaves_the_transcript_alone() {
        let mut messages = chat(2);
        messages.push(marker("s".into(), MessageId::new(), 4, Vec::new()));
        assert_eq!(live(&messages).len(), 5, "nothing is dropped on a guess");
    }

    #[test]
    fn the_cut_lands_on_a_turn_boundary_and_keeps_the_last_turns() {
        let messages = chat(6);
        let kept = span(&messages, 3).expect("six turns, keep three");
        assert_eq!(kept, 0..6);
        assert_eq!(
            messages[kept.end].role,
            Role::User,
            "the cut is before a user message, so no call is split from its result"
        );
        // Anthropic keeps nothing: the summary replaces the whole history (02 §6).
        assert_eq!(span(&messages, 0), Some(0..12));
    }

    #[test]
    fn a_short_chat_is_left_alone() {
        assert_eq!(span(&chat(3), 3), None, "nothing older than what we keep");
        assert_eq!(span(&chat(1), 0), None, "and nothing worth a model call");
    }

    #[test]
    fn the_provider_decides_how_much_survives() {
        assert_eq!(keep_turns(ProviderKind::Anthropic), 0);
        assert_eq!(keep_turns(ProviderKind::OpenAiChat), KEEP_TURNS);
    }

    #[test]
    fn measured_usage_beats_counting_characters() {
        let messages = chat(2);
        let usage = Usage {
            input: 40_000,
            output: 500,
            cache_read: 1_000,
            ..Default::default()
        };
        // What the provider charged already includes the prompt and the tools, so the counted
        // overhead is not added on top of it.
        assert_eq!(estimate(&messages, 900, Some(&usage)), 41_500);
        // No usage, or a provider that reported none: count, and add what the request carries
        // besides the transcript.
        assert_eq!(estimate(&messages, 900, None), counted(&messages) + 900);
        assert_eq!(
            estimate(&messages, 0, Some(&Usage::default())),
            counted(&messages)
        );
    }

    #[test]
    fn the_tool_schemas_are_counted_too() {
        use gantry_providers::ToolSpec;
        let spec = ToolSpec {
            name: "filesystem__read_file".into(),
            description: "d".repeat(200),
            input_schema: serde_json::json!({ "type": "object" }),
            strict: false,
            deferred: false,
            stream_args: false,
        };
        let system = "s".repeat(4_000);
        assert_eq!(overhead(&system, &[]), 1_000);
        assert!(
            overhead(&system, std::slice::from_ref(&spec)) > 1_050,
            "a tool is not free"
        );
    }

    #[test]
    fn the_budget_is_three_quarters_of_the_window() {
        assert!(!over_budget(99_000, Some(200_000)));
        assert!(over_budget(151_000, Some(200_000)));
        // A window nobody knows still has a budget, and zero is not a window.
        assert!(over_budget(100_000, None));
        assert!(over_budget(100_000, Some(0)));
    }

    #[test]
    fn a_tool_loop_is_rendered_as_what_it_did() {
        let call = Message {
            id: MessageId::new(),
            role: Role::Assistant,
            parts: vec![ContentPart::ToolCall {
                id: CallId::new(),
                name: "shell__run_command".into(),
                args: serde_json::json!({ "command": "cargo test" }),
                signature: None,
            }],
            origin: None,
            created_at: 0,
        };
        let result = Message {
            id: MessageId::new(),
            role: Role::Tool,
            parts: vec![ContentPart::ToolResult {
                call_id: CallId::new(),
                content: vec![ResultPart::Text {
                    text: "x".repeat(5_000),
                }],
                is_error: false,
            }],
            origin: None,
            created_at: 0,
        };
        let text = render(&[user("run the tests"), call, result]);
        assert!(text.contains("User: run the tests"), "{text}");
        assert!(text.contains("shell__run_command"), "{text}");
        assert!(text.contains("cargo test"), "{text}");
        assert!(
            text.len() < 2_000,
            "the output the compaction exists to remove is not handed to the summarizer"
        );
    }

    #[test]
    fn artifacts_survive_the_span_that_made_them() {
        let made = Message {
            id: MessageId::new(),
            role: Role::Assistant,
            parts: vec![
                ContentPart::ToolCall {
                    id: CallId::new(),
                    name: "gantry__create_artifact".into(),
                    args: serde_json::json!({ "title": "Sales dashboard", "type": "react" }),
                    signature: None,
                },
                ContentPart::ToolCall {
                    id: CallId::new(),
                    name: "gantry__update_artifact".into(),
                    args: serde_json::json!({ "artifact_id": "a-123" }),
                    signature: None,
                },
                ContentPart::ToolCall {
                    id: CallId::new(),
                    name: "shell__run_command".into(),
                    args: serde_json::json!({ "command": "ls" }),
                    signature: None,
                },
            ],
            origin: None,
            created_at: 0,
        };
        assert_eq!(artifacts_in(&[made]), vec!["Sales dashboard", "a-123"]);
    }

    #[test]
    fn the_marker_tells_the_model_what_it_can_still_reach() {
        let m = marker(
            "we built a thing".into(),
            MessageId::new(),
            12,
            vec!["a-1".into()],
        );
        let text = m.parts[0]
            .system_text()
            .expect("a marker speaks to the model");
        assert!(text.contains("12 messages"), "{text}");
        assert!(text.contains("we built a thing"), "{text}");
        assert!(text.contains("gantry__read_artifact"), "{text}");
        assert!(text.contains("a-1"), "{text}");
    }
}
