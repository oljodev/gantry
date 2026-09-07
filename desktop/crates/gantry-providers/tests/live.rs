//! Opt-in live smoke test against OpenRouter. Costs a fraction of a cent. Run with
//! `OPENROUTER_API_KEY=… cargo test -p gantry-providers --test live -- --ignored`.

use futures_util::StreamExt;
use gantry_core::{Message, ProviderId, ReasoningEffort};
use gantry_providers::{
    ChatRequest, CompatProfile, OpenAiChatProvider, Provider, StreamEvent, openai_chat::http_client,
};
use gantry_secrets::SecretString;

fn provider() -> Option<OpenAiChatProvider> {
    let key = std::env::var("OPENROUTER_API_KEY").ok()?;
    Some(OpenAiChatProvider::new(
        ProviderId::openrouter(),
        CompatProfile::openrouter(),
        Some(SecretString::from(key)),
        http_client("test"),
        Vec::new(),
    ))
}

#[tokio::test]
#[ignore = "needs OPENROUTER_API_KEY and spends a fraction of a cent"]
async fn key_models_and_a_short_stream() {
    let Some(p) = provider() else {
        eprintln!("OPENROUTER_API_KEY not set");
        return;
    };
    let key = p.check_key().await.unwrap();
    eprintln!("key: {key:?}");
    let models = p.list_models().await.unwrap();
    assert!(models.iter().any(|m| m.id == "deepseek/deepseek-v4-flash"));

    let mut req = ChatRequest::new(
        "deepseek/deepseek-v4-flash",
        "Answer in at most one short sentence.",
        vec![Message::user_text("What is a gantry crane?")],
    );
    req.max_output_tokens = 120;
    req.reasoning = ReasoningEffort::Low;
    let mut stream = p.stream(req).await.unwrap();
    let mut text = String::new();
    let mut ended = false;
    while let Some(ev) = stream.next().await {
        match ev.unwrap() {
            StreamEvent::TextDelta { text: t, .. } => text.push_str(&t),
            StreamEvent::MessageEnd { .. } => ended = true,
            _ => {}
        }
    }
    eprintln!("answer: {text}");
    assert!(ended && !text.is_empty());
}
