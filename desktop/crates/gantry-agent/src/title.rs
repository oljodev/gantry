//! The title generator (docs/plan/01 §3 step 7): after a chat's first exchange, the cheapest
//! fast model of the same provider names it. The judge (M8) uses the same table.

use std::sync::Arc;

use futures_util::StreamExt;
use gantry_core::{Message, ProviderKind, ReasoningEffort};
use gantry_providers::{ChatRequest, Provider, ProviderError, StreamEvent};

const JUDGE_DEFAULTS: &str = include_str!("../../../assets/models/judge_defaults.toml");

/// Longest title kept, in characters.
pub const MAX_TITLE_CHARS: usize = 60;

#[derive(Debug, serde::Deserialize, Default)]
struct JudgeFile {
    #[serde(default)]
    judge: std::collections::BTreeMap<String, String>,
}

/// The judge model for a provider: by provider id first, then by client kind, else the
/// chat's own model.
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
    req.max_output_tokens = 32;
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
