//! The title generator (docs/plan/01 §3 step 7): after a chat's first exchange, a small model
//! names it. The guard (M8) and the compactor (02 §6) ask the same model.
//!
//! Which model that is, is one setting — Settings → Guard's **Utility model**. Unset, it is the
//! cheapest fast model of the chat's own provider, from `judge_defaults.toml`, so no second key
//! is needed. Set, it is that model for all three, because a user who does not want a given
//! model called on their behalf means all of the calls, not the guard's alone.

use std::sync::Arc;

use futures_util::StreamExt;
use gantry_core::{Message, ModelRef, ProviderId, ProviderKind, ReasoningEffort};
use gantry_providers::{ChatRequest, Provider, ProviderError, StreamEvent};

const JUDGE_DEFAULTS: &str = include_str!("../../../assets/models/judge_defaults.toml");

/// Longest title kept, in characters.
pub const MAX_TITLE_CHARS: usize = 60;

#[derive(Debug, serde::Deserialize, Default)]
struct JudgeFile {
    #[serde(default)]
    judge: std::collections::BTreeMap<String, String>,
}

/// The shipped judge model for a provider: by provider id first, then by client kind, else the
/// chat's own model. [`resolve_judge`] is what callers want — this is the default it falls to.
#[must_use]
pub fn judge_model(provider_id: &str, kind: ProviderKind, fallback: &str) -> String {
    let file: JudgeFile = toml::from_str(JUDGE_DEFAULTS).unwrap_or_default();
    let kind_key = match kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenAiResponses => "openai_responses",
        ProviderKind::OpenAiChat => "openai_chat",
        ProviderKind::Gemini => "gemini",
    };
    file.judge
        .get(provider_id)
        .or_else(|| file.judge.get(kind_key))
        .cloned()
        .unwrap_or_else(|| fallback.to_owned())
}

/// The provider and model every utility call uses: naming a chat, compacting a transcript and
/// the guard's decisions.
///
/// `chosen` is the user's override (`settings.guard.judge_model`). It carries its own provider,
/// because a model id means nothing without the provider that serves it, and that provider has
/// to be one Gantry has a client for — an override naming a provider with no key resolves to
/// `None` rather than silently falling back to a model the user has said no to.
#[must_use]
pub fn resolve_judge(
    chosen: Option<&ModelRef>,
    lookup: &dyn Fn(&ProviderId) -> Option<Arc<dyn Provider>>,
    chat_provider: Option<&Arc<dyn Provider>>,
    chat_model: &ModelRef,
) -> Option<(Arc<dyn Provider>, String)> {
    match chosen {
        Some(chosen) => lookup(&chosen.provider).map(|p| (p, chosen.model.clone())),
        None => chat_provider.map(|p| {
            let m = judge_model(chat_model.provider.as_str(), p.kind(), &chat_model.model);
            (p.clone(), m)
        }),
    }
}

const SYSTEM: &str = "You name conversations. Reply with a title of at most six words for the \
conversation below: no quotes, no trailing period, in the language the user wrote in. Reply \
with the title only.";

/// One short non-streaming request. Costs a few hundred tokens.
pub async fn generate_title(
    provider: Arc<dyn Provider>,
    model: String,
    user_text: &str,
    assistant_text: &str,
) -> Result<String, ProviderError> {
    let excerpt = |s: &str, n: usize| -> String { s.chars().take(n).collect() };
    let prompt = format!(
        "User:\n{}\n\nAssistant:\n{}\n\nTitle:",
        excerpt(user_text, 1200),
        excerpt(assistant_text, 800)
    );
    let mut req = ChatRequest::new(model, SYSTEM, vec![Message::user_text(prompt)]);
    req.max_output_tokens = 64;
    req.reasoning = ReasoningEffort::Off;
    let mut stream = provider.stream(req).await?;
    let mut text = String::new();
    while let Some(ev) = stream.next().await {
        match ev? {
            StreamEvent::TextDelta { text: t, .. } => text.push_str(&t),
            StreamEvent::MessageEnd { .. } => break,
            _ => {}
        }
    }
    Ok(clean_title(&text))
}

/// First line, quotes and trailing punctuation stripped, capped at [`MAX_TITLE_CHARS`].
#[must_use]
pub fn clean_title(raw: &str) -> String {
    let line = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let line = line.trim_start_matches("Title:").trim();
    let line = line.trim_matches(|c: char| matches!(c, '"' | '\'' | '“' | '”' | '«' | '»' | '*'));
    let line = line.trim_end_matches(['.', '。']).trim();
    let mut out: String = line.chars().take(MAX_TITLE_CHARS).collect();
    if line.chars().count() > MAX_TITLE_CHARS {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_cleaned() {
        assert_eq!(
            clean_title("\"WAL mode explained.\"\n"),
            "WAL mode explained"
        );
        assert_eq!(clean_title("Title: **Hei**"), "Hei");
        assert_eq!(clean_title("   \n"), "");
        let long = "x".repeat(80);
        assert_eq!(clean_title(&long).chars().count(), MAX_TITLE_CHARS + 1);
    }

    #[test]
    fn openrouter_uses_deepseek_flash_and_unknown_providers_fall_back() {
        assert_eq!(
            judge_model("openrouter", ProviderKind::OpenAiChat, "x"),
            "deepseek/deepseek-v4-flash"
        );
        assert_eq!(
            judge_model("custom:abc", ProviderKind::Gemini, "own-model"),
            "own-model"
        );
    }
}
