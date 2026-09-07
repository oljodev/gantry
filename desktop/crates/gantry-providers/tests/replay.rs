//! Every fixture under `tests/fixtures/{anthropic,openai_responses,gemini}/` fed through the
//! SSE decoder in odd-sized chunks and the provider's parser, asserting the normalized events
//! (docs/plan/02 §8). One test per row of the normalization table that concerns streaming.

use std::{path::PathBuf, time::Duration};

use futures_util::StreamExt;
use gantry_core::{ContentPart, ProviderErrorKind, ProviderKind, StopReason, Usage};
use gantry_providers::{ProviderError, StreamEvent, sse::sse_stream};

type Events = Vec<Result<StreamEvent, ProviderError>>;

fn fixture(provider: &str, name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(provider)
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

async fn replay(provider: &str, name: &str, cut: Option<usize>) -> Events {
    let mut bytes = fixture(provider, name);
    if let Some(n) = cut {
        bytes.truncate(n);
    }
    let chunks: Vec<Result<Vec<u8>, ProviderError>> =
        bytes.chunks(7).map(|c| Ok(c.to_vec())).collect();
    let sse = sse_stream(
        futures_util::stream::iter(chunks),
        Duration::from_secs(1),
        Duration::from_secs(1),
    );
    let chat = match provider {
        "anthropic" => gantry_providers::anthropic::stream::into_chat_stream(sse),
        "openai_responses" => gantry_providers::openai_responses::stream::into_chat_stream(sse),
        "gemini" => gantry_providers::gemini::stream::into_chat_stream(sse),
        other => panic!("no parser for {other}"),
    };
    chat.collect().await
}

fn text_of(events: &Events) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            Ok(StreamEvent::TextDelta { text, .. }) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn thinking_of(events: &Events) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            Ok(StreamEvent::ThinkingDelta { text, .. }) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn usage_of(events: &Events) -> Usage {
    events
        .iter()
        .find_map(|e| match e {
            Ok(StreamEvent::Usage(u)) => Some(*u),
            _ => None,
        })
        .expect("usage")
}

fn stop_of(events: &Events) -> StopReason {
    match events.last() {
        Some(Ok(StreamEvent::MessageEnd { stop_reason })) => stop_reason.clone(),
        other => panic!("no message end: {other:?}"),
    }
}

fn arg_deltas(events: &Events, index: u32) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Ok(StreamEvent::ToolCallArgsDelta {
                index: i,
                json_fragment,
            }) if *i == index => Some(json_fragment.clone()),
            _ => None,
        })
        .collect()
}

fn tool_end(events: &Events, index: u32) -> Vec<serde_json::Value> {
    events
        .iter()
        .filter_map(|e| match e {
            Ok(StreamEvent::ToolCallEnd { index: i, args }) if *i == index => Some(args.clone()),
            _ => None,
        })
        .collect()
}

fn tool_start(events: &Events, index: u32) -> (String, String) {
    events
        .iter()
        .find_map(|e| match e {
            Ok(StreamEvent::ToolCallStart { index: i, id, name }) if *i == index => {
                Some((id.as_str().to_owned(), name.clone()))
            }
            _ => None,
        })
        .expect("tool call start")
}

fn blocks(events: &Events) -> Vec<(u32, ContentPart)> {
    events
        .iter()
        .filter_map(|e| match e {
            Ok(StreamEvent::ProviderBlock { index, part }) => Some((*index, part.clone())),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Anthropic
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_text_usage_and_stop() {
    let events = replay("anthropic", "text.sse", None).await;
    assert!(
        matches!(&events[0], Ok(StreamEvent::MessageStart { provider_message_id: Some(id) }) if id == "msg_01")
    );
    assert_eq!(text_of(&events), "Hello, world.");
    let u = usage_of(&events);
    assert_eq!((u.input, u.output), (25, 4));
    assert_eq!(stop_of(&events), StopReason::EndTurn);
    assert!(events.iter().all(Result::is_ok));
}

#[tokio::test]
async fn anthropic_thinking_signature_and_parallel_tool_calls_stream() {
    let events = replay("anthropic", "thinking_tools.sse", None).await;
    assert_eq!(
        thinking_of(&events),
        "The user wants the time and the week."
    );
    assert!(events.iter().any(|e| matches!(e,
        Ok(StreamEvent::ThinkingSignature { index: 0, signature }) if signature == "SIG_abc123")));
    assert_eq!(text_of(&events), "Let me check both.");
    assert_eq!(
        tool_start(&events, 2),
        ("toolu_01A".into(), "gantry__clock".into())
    );
    assert_eq!(arg_deltas(&events, 2), ["{\"zone\": ", "\"local\"}"]);
    assert_eq!(
        tool_end(&events, 2),
        [serde_json::json!({ "zone": "local" })]
    );
    assert_eq!(tool_start(&events, 3).0, "toolu_01B");
    assert_eq!(tool_end(&events, 3), [serde_json::json!({})]);
    let u = usage_of(&events);
    assert_eq!(
        (u.input, u.output, u.cache_read, u.cache_write),
        (1300, 61, 1000, 200)
    );
    assert_eq!(stop_of(&events), StopReason::ToolUse);
}

#[tokio::test]
async fn anthropic_refusal_and_max_tokens_are_stop_reasons_not_errors() {
    let events = replay("anthropic", "refusal.sse", None).await;
    assert_eq!(text_of(&events), "I can't help with that.");
    assert_eq!(
        stop_of(&events),
        StopReason::Refusal {
            category: Some("cbrn".into())
        }
    );
    let events = replay("anthropic", "max_tokens.sse", None).await;
    assert_eq!(stop_of(&events), StopReason::MaxTokens);
}

#[tokio::test]
async fn anthropic_error_event_ends_the_stream_with_its_class() {
    let events = replay("anthropic", "error_midstream.sse", None).await;
    assert_eq!(text_of(&events), "Partial");
    let err = events.last().unwrap().as_ref().unwrap_err();
    assert_eq!(err.kind, ProviderErrorKind::Overloaded);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Ok(StreamEvent::MessageEnd { .. })))
    );
}

#[tokio::test]
async fn anthropic_server_tool_blocks_are_opaque_and_replayable() {
    let events = replay("anthropic", "web_search.sse", None).await;
    let blocks = blocks(&events);
    assert_eq!(blocks.len(), 2);
    match &blocks[0].1 {
        ContentPart::ProviderOpaque {
            provider: ProviderKind::Anthropic,
            block_kind,
            json,
        } => {
            assert_eq!(block_kind, "server_tool_use");
            assert_eq!(json["input"]["query"], "gantry crane");
            assert_eq!(json["id"], "srvtoolu_01");
        }
        other => panic!("{other:?}"),
    }
    assert!(
        matches!(&blocks[1].1, ContentPart::ProviderOpaque { block_kind, .. } if block_kind == "web_search_tool_result")
    );
    assert_eq!(text_of(&events), "A gantry crane straddles its load.");
    assert_eq!(stop_of(&events), StopReason::EndTurn);
}

#[tokio::test]
async fn anthropic_early_close_keeps_text_and_reports_interruption() {
    let full = fixture("anthropic", "text.sse");
    let cut = full
        .windows(b"event: message_delta".len())
        .position(|w| w == b"event: message_delta")
        .unwrap();
    let events = replay("anthropic", "text.sse", Some(cut)).await;
    assert_eq!(text_of(&events), "Hello, world.");
    let err = events.last().unwrap().as_ref().unwrap_err();
    assert_eq!(err.kind, ProviderErrorKind::StreamInterrupted);
}

// ---------------------------------------------------------------------------------------------
// OpenAI Responses
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn responses_text_usage_and_stop() {
    let events = replay("openai_responses", "text.sse", None).await;
    assert!(
        matches!(&events[0], Ok(StreamEvent::MessageStart { provider_message_id: Some(id) }) if id == "resp_01")
    );
    assert_eq!(text_of(&events), "Hello, world.");
    let u = usage_of(&events);
    assert_eq!((u.input, u.output, u.cache_read), (12, 4, 8));
    assert_eq!(stop_of(&events), StopReason::EndTurn);
    assert!(events.iter().all(Result::is_ok));
}

#[tokio::test]
async fn responses_reasoning_item_becomes_one_thinking_part_with_id_and_encrypted_content() {
    let events = replay("openai_responses", "reasoning_tools.sse", None).await;
    assert_eq!(thinking_of(&events), "**Checking the clock**");
    let blocks = blocks(&events);
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0],
        (
            0,
            ContentPart::Thinking {
                text: "**Checking the clock**".into(),
                signature: Some("ENC_xyz".into()),
                provider: ProviderKind::OpenAiResponses,
                item_id: Some("rs_01".into()),
            }
        )
    );
    assert_eq!(
        tool_start(&events, 1),
        ("call_A1".into(), "gantry__clock".into())
    );
    assert_eq!(arg_deltas(&events, 1), ["{\"zone\":", "\"local\"}"]);
    assert_eq!(
        tool_end(&events, 1),
        [serde_json::json!({ "zone": "local" })],
        "the call ends once, on arguments.done"
    );
    assert_eq!(tool_start(&events, 2).0, "call_A2");
    assert_eq!(tool_end(&events, 2), [serde_json::json!({})]);
    let u = usage_of(&events);
    assert_eq!(
        (u.input, u.cache_read, u.output, u.reasoning),
        (200, 128, 90, 64)
    );
    assert_eq!(stop_of(&events), StopReason::ToolUse);
}

#[tokio::test]
async fn responses_incomplete_and_error_are_classified() {
    let events = replay("openai_responses", "incomplete.sse", None).await;
    assert_eq!(text_of(&events), "Once upon a");
    assert_eq!(stop_of(&events), StopReason::MaxTokens);
    let events = replay("openai_responses", "error_midstream.sse", None).await;
    assert_eq!(text_of(&events), "Partial");
    let err = events.last().unwrap().as_ref().unwrap_err();
    assert_eq!(err.kind, ProviderErrorKind::RateLimited);
}

#[tokio::test]
async fn responses_web_search_call_is_opaque() {
    let events = replay("openai_responses", "web_search.sse", None).await;
    let blocks = blocks(&events);
    assert_eq!(blocks.len(), 1);
    match &blocks[0].1 {
        ContentPart::ProviderOpaque {
            provider: ProviderKind::OpenAiResponses,
            block_kind,
            json,
        } => {
            assert_eq!(block_kind, "web_search_call");
            assert_eq!(json["action"]["query"], "gantry crane");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(text_of(&events), "A gantry crane straddles its load.");
    assert_eq!(stop_of(&events), StopReason::EndTurn);
}

// ---------------------------------------------------------------------------------------------
// Gemini
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn gemini_text_usage_and_stop() {
    let events = replay("gemini", "text.sse", None).await;
    assert!(
        matches!(&events[0], Ok(StreamEvent::MessageStart { provider_message_id: Some(id) }) if id == "int_01")
    );
    assert_eq!(text_of(&events), "Hello, world.");
    let u = usage_of(&events);
    assert_eq!((u.input, u.output), (12, 4));
    assert_eq!(stop_of(&events), StopReason::EndTurn);
    assert!(events.iter().all(Result::is_ok));
}

#[tokio::test]
async fn gemini_thought_signatures_land_on_their_steps_and_arguments_stream() {
    let events = replay("gemini", "thought_tools.sse", None).await;
    assert_eq!(thinking_of(&events), "Need the time.");
    assert!(events.iter().any(|e| matches!(e,
        Ok(StreamEvent::ThinkingSignature { index: 0, signature }) if signature == "TS_thought")));
    assert_eq!(
        tool_start(&events, 1),
        ("fc-1".into(), "gantry__clock".into())
    );
    assert_eq!(arg_deltas(&events, 1), ["{\"zone\":", "\"local\"}"]);
    assert_eq!(
        tool_end(&events, 1),
        [serde_json::json!({ "zone": "local" })]
    );
    let blocks = blocks(&events);
    assert_eq!(
        blocks,
        [(
            1,
            ContentPart::ToolCall {
                id: gantry_core::CallId("fc-1".into()),
                name: "gantry__clock".into(),
                args: serde_json::json!({ "zone": "local" }),
                signature: Some("TS_call".into()),
            }
        )],
        "the signed call is re-issued whole so the transcript keeps the signature"
    );
    assert_eq!(tool_start(&events, 2).0, "fc-2");
    assert!(
        arg_deltas(&events, 2).is_empty(),
        "delivered whole, no deltas"
    );
    assert_eq!(tool_end(&events, 2), [serde_json::json!({ "zone": "utc" })]);
    let u = usage_of(&events);
    assert_eq!(
        (u.input, u.output, u.cache_read, u.reasoning),
        (300, 40, 100, 25)
    );
    assert_eq!(stop_of(&events), StopReason::ToolUse);
}

#[tokio::test]
async fn gemini_error_event_is_classified() {
    let events = replay("gemini", "error_midstream.sse", None).await;
    assert_eq!(text_of(&events), "Partial");
    let err = events.last().unwrap().as_ref().unwrap_err();
    assert_eq!(err.kind, ProviderErrorKind::RateLimited);
}
