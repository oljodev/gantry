//! Every fixture under `tests/fixtures/openrouter/` fed through the SSE decoder in odd-sized
//! chunks and the chunk parser, asserting the normalized events (docs/plan/02 §8).

use std::{path::PathBuf, time::Duration};

use futures_util::StreamExt;
use gantry_core::{ProviderErrorKind, StopReason};
use gantry_providers::{
    ProviderError, StreamEvent,
    openai_chat::ToolIdQuirk,
    sse::{SseDecoder, sse_stream},
};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/openrouter")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Runs a fixture through the production path: byte chunks → SSE → chunk parser → events.
async fn replay(name: &str) -> Vec<Result<StreamEvent, ProviderError>> {
    let bytes = fixture(name);
    let chunks: Vec<Result<Vec<u8>, ProviderError>> =
        bytes.chunks(7).map(|c| Ok(c.to_vec())).collect();
    let byte_stream = futures_util::stream::iter(chunks);
    let sse = sse_stream(byte_stream, Duration::from_secs(1), Duration::from_secs(1));
    let chat = gantry_providers::openai_chat::stream::into_chat_stream(
        sse,
        ToolIdQuirk::SynthesizeIfEmpty,
    );
    chat.collect().await
}

fn text_of(events: &[Result<StreamEvent, ProviderError>]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            Ok(StreamEvent::TextDelta { text, .. }) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn text_stream_normalizes_deltas_usage_and_stop() {
    let events = replay("text.sse").await;
    assert!(
        matches!(&events[0], Ok(StreamEvent::MessageStart { provider_message_id: Some(id) }) if id == "gen-1")
    );
    assert_eq!(text_of(&events), "Hello, world.");
    let usage = events.iter().find_map(|e| match e {
        Ok(StreamEvent::Usage(u)) => Some(*u),
        _ => None,
    });
    let usage = usage.expect("usage from the final chunk");
    assert_eq!((usage.input, usage.output, usage.cache_read), (12, 4, 8));
    assert!((usage.cost_usd.unwrap() - 0.0000018).abs() < 1e-12);
    assert!(matches!(
        events.last(),
        Some(Ok(StreamEvent::MessageEnd {
            stop_reason: StopReason::EndTurn
        }))
    ));
    assert!(events.iter().all(Result::is_ok));
}

#[tokio::test]
async fn reasoning_precedes_text_in_its_own_block() {
    let events = replay("reasoning.sse").await;
    let thinking: String = events
        .iter()
        .filter_map(|e| match e {
            Ok(StreamEvent::ThinkingDelta { text, index }) => {
                assert_eq!(*index, 0);
                Some(text.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(thinking, "Let me think.");
    assert_eq!(text_of(&events), "Water is heavier.");
    let text_index = events.iter().find_map(|e| match e {
        Ok(StreamEvent::TextDelta { index, .. }) => Some(*index),
        _ => None,
    });
    assert_eq!(text_index, Some(1));
    let usage = events.iter().find_map(|e| match e {
        Ok(StreamEvent::Usage(u)) => Some(*u),
        _ => None,
    });
    assert_eq!(usage.unwrap().reasoning, 25);
}

#[tokio::test]
async fn a_mid_stream_error_ends_the_stream_with_a_classified_error() {
    let events = replay("error_midstream.sse").await;
    assert_eq!(text_of(&events), "Part");
    let err = events
        .iter()
        .find_map(|e| e.as_ref().err())
        .expect("an error");
    assert_eq!(err.kind, ProviderErrorKind::Overloaded);
    assert!(err.message.contains("DeepSeek"), "{}", err.message);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Ok(StreamEvent::MessageEnd { .. })))
    );
}

#[tokio::test]
async fn an_early_close_keeps_the_text_then_reports_interruption() {
    let events = replay("no_usage_early_close.sse").await;
    assert_eq!(text_of(&events), "Unfinished");
    let last = events.last().unwrap();
    assert!(
        matches!(last, Err(e) if e.kind == ProviderErrorKind::StreamInterrupted),
        "{last:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Ok(StreamEvent::MessageEnd { .. })))
    );
}

#[tokio::test]
async fn length_maps_to_max_tokens() {
    let events = replay("length.sse").await;
    assert!(matches!(
        events.last(),
        Some(Ok(StreamEvent::MessageEnd {
            stop_reason: StopReason::MaxTokens
        }))
    ));
}

#[tokio::test]
async fn tool_calls_stream_arguments_and_end_with_parsed_args() {
    let events = replay("tool_call.sse").await;
    let start = events.iter().find_map(|e| match e {
        Ok(StreamEvent::ToolCallStart { id, name, index }) => {
            Some((id.clone(), name.clone(), *index))
        }
        _ => None,
    });
    let (id, name, index) = start.expect("a tool call start");
    assert!(
        id.as_str().starts_with("gantry_"),
        "empty ids are synthesized: {id}"
    );
    assert_eq!(name, "filesystem__read_file");
    let end = events.iter().find_map(|e| match e {
        Ok(StreamEvent::ToolCallEnd { args, index: i }) if *i == index => Some(args.clone()),
        _ => None,
    });
    assert_eq!(end.unwrap()["path"], "src/main.rs");
    assert!(matches!(
        events.last(),
        Some(Ok(StreamEvent::MessageEnd {
            stop_reason: StopReason::ToolUse
        }))
    ));
}

#[tokio::test]
async fn the_live_capture_parses_when_present() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/openrouter/live-capture.sse");
    if !path.exists() {
        eprintln!("no live capture yet; skipping");
        return;
    }
    let events = replay("live-capture.sse").await;
    assert!(events.iter().all(Result::is_ok), "{events:?}");
    assert!(!text_of(&events).is_empty());
    assert!(matches!(
        events.last(),
        Some(Ok(StreamEvent::MessageEnd { .. }))
    ));
}

#[test]
fn the_decoder_survives_a_chunk_boundary_inside_a_utf8_character() {
    let mut d = SseDecoder::new();
    let text = "data: {\"content\":\"héllo\"}\n\n".as_bytes();
    let mut events = Vec::new();
    for c in text.chunks(3) {
        events.extend(d.push(c));
    }
    assert_eq!(events.len(), 1);
    assert!(events[0].data.contains("héllo"));
}
