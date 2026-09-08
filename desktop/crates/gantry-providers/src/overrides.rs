//! `desktop/assets/models/overrides.toml`: what the model lists do not say (docs/plan/02 §2).
//!
//! Each `[[model]]` entry names a provider (an account id such as `anthropic`, or a client kind
//! such as `openai_chat`) and a model id, exact or with a trailing `*`. Entries apply in file
//! order, so a specific id placed after a pattern refines it. Only the fields an entry sets are
//! changed; everything else stays what the provider's list said.

use gantry_core::ProviderKind;
use serde::Deserialize;

use crate::provider::{CacheSupport, ModelInfo, Pricing, ReasoningSupport};

const OVERRIDES: &str = include_str!("../../../assets/models/overrides.toml");

#[derive(Debug, Default, Deserialize)]
struct File {
    #[serde(default)]
    model: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    provider: String,
    id: String,
    display_name: Option<String>,
    context_window: Option<u32>,
    max_output: Option<u32>,
    thinking: Option<Thinking>,
    web_search: Option<bool>,
    tools: Option<bool>,
    parallel_tools: Option<bool>,
    streams_tool_args: Option<bool>,
    vision: Option<bool>,
    pdf_input: Option<bool>,
    structured_output: Option<bool>,
    prompt_caching: Option<String>,
    input_price_per_mtok: Option<f64>,
    output_price_per_mtok: Option<f64>,
    cache_read_price_per_mtok: Option<f64>,
}

/// `thinking = true` means "takes an effort level"; a string picks the exact style.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Thinking {
    Flag(bool),
    Style(String),
}

impl Thinking {
    fn support(&self) -> ReasoningSupport {
        match self {
            Self::Flag(true) => ReasoningSupport::Effort,
            Self::Flag(false) => ReasoningSupport::None,
            Self::Style(s) => match s.as_str() {
                "budget" => ReasoningSupport::Budget,
                "effort" | "adaptive" => ReasoningSupport::Effort,
                _ => ReasoningSupport::None,
            },
        }
    }
}

fn kind_key(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenAiResponses => "openai_responses",
        ProviderKind::OpenAiChat => "openai_chat",
        ProviderKind::Gemini => "gemini",
    }
}

fn matches_id(pattern: &str, id: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => id.starts_with(prefix),
        None => pattern == id,
    }
}

/// Parses the shipped file once per call; it is a few kilobytes and this runs per model list.
fn entries() -> Vec<Entry> {
    match toml::from_str::<File>(OVERRIDES) {
        Ok(f) => f.model,
        Err(err) => {
            log::error!("overrides.toml does not parse: {err}");
            Vec::new()
        }
    }
}

/// Applies every matching entry to `m`.
pub fn apply(kind: ProviderKind, provider_id: &str, m: &mut ModelInfo) {
    apply_entries(&entries(), kind, provider_id, m);
}

/// Applies the overrides to a whole list.
pub fn apply_all(kind: ProviderKind, provider_id: &str, list: &mut [ModelInfo]) {
    let entries = entries();
    for m in list {
        apply_entries(&entries, kind, provider_id, m);
    }
}

fn apply_entries(entries: &[Entry], kind: ProviderKind, provider_id: &str, m: &mut ModelInfo) {
    let kind_key = kind_key(kind);
    for e in entries
        .iter()
        .filter(|e| e.provider == provider_id || e.provider == kind_key)
        .filter(|e| matches_id(&e.id, &m.id))
    {
        if let Some(n) = &e.display_name {
            m.display_name = n.clone();
        }
        if e.context_window.is_some() {
            m.context_window = e.context_window;
        }
        if e.max_output.is_some() {
            m.max_output = e.max_output;
        }
        if let Some(t) = &e.thinking {
            m.capabilities.reasoning = t.support();
        }
        if let Some(v) = e.web_search {
            m.capabilities.server_web_search = v;
        }
        if let Some(v) = e.tools {
            m.capabilities.tools = v;
        }
        if let Some(v) = e.parallel_tools {
            m.capabilities.parallel_tools = v;
        }
        if let Some(v) = e.streams_tool_args {
            m.capabilities.streams_tool_args = v;
        }
        if let Some(v) = e.vision {
            m.capabilities.vision = v;
        }
        if let Some(v) = e.pdf_input {
            m.capabilities.pdf_input = v;
        }
        if let Some(v) = e.structured_output {
            m.capabilities.structured_output = v;
        }
        if let Some(c) = e.prompt_caching.as_deref() {
            m.capabilities.prompt_caching = match c {
                "explicit" => CacheSupport::Explicit,
                "automatic" => CacheSupport::Automatic,
                _ => CacheSupport::None,
            };
        }
        if e.input_price_per_mtok.is_some() || e.output_price_per_mtok.is_some() {
            let current = m.pricing;
            m.pricing = Some(Pricing {
                input_per_mtok: e
                    .input_price_per_mtok
                    .or(current.map(|p| p.input_per_mtok))
                    .unwrap_or(0.0),
                output_per_mtok: e
                    .output_price_per_mtok
                    .or(current.map(|p| p.output_per_mtok))
                    .unwrap_or(0.0),
                cache_read_per_mtok: e
                    .cache_read_price_per_mtok
                    .or(current.and_then(|p| p.cache_read_per_mtok)),
                // The override file speaks in token prices; whatever the provider said about
                // per-image and per-call prices survives it.
                image_input_usd: current.and_then(|p| p.image_input_usd),
                image_output_usd: current.and_then(|p| p.image_output_usd),
                request_usd: current.and_then(|p| p.request_usd),
            });
        } else if let Some(v) = e.cache_read_price_per_mtok
            && let Some(p) = m.pricing.as_mut()
        {
            p.cache_read_per_mtok = Some(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ModelCapabilities;

    fn bare(id: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            display_name: id.into(),
            created_at: None,
            context_window: None,
            max_output: None,
            pricing: None,
            capabilities: ModelCapabilities::default(),
        }
    }

    #[test]
    fn the_shipped_file_parses_and_describes_the_known_models() {
        assert!(!entries().is_empty(), "overrides.toml has entries");
        let mut m = bare("claude-sonnet-4-5");
        apply(ProviderKind::Anthropic, "anthropic", &mut m);
        assert_eq!(m.context_window, Some(200_000));
        assert_eq!(m.capabilities.reasoning, ReasoningSupport::Budget);
        assert!(m.capabilities.server_web_search);
        assert_eq!(m.capabilities.prompt_caching, CacheSupport::Explicit);

        let mut m = bare("claude-opus-5");
        apply(ProviderKind::Anthropic, "anthropic", &mut m);
        assert_eq!(m.capabilities.reasoning, ReasoningSupport::Effort);

        let mut m = bare("gpt-5-mini");
        apply(ProviderKind::OpenAiResponses, "openai", &mut m);
        assert_eq!(m.context_window, Some(400_000));
        assert!(m.capabilities.streams_tool_args);
        let p = m.pricing.unwrap();
        assert!((p.input_per_mtok - 0.25).abs() < 1e-9);

        let mut m = bare("gemini-2.5-flash");
        apply(ProviderKind::Gemini, "google", &mut m);
        assert_eq!(m.context_window, Some(1_048_576));
        assert!(m.capabilities.vision);

        let mut m = bare("grok-4-fast-reasoning");
        apply(ProviderKind::OpenAiChat, "xai", &mut m);
        assert_eq!(m.capabilities.reasoning, ReasoningSupport::Effort);
        assert_eq!(m.context_window, Some(2_000_000));
    }

    #[test]
    fn later_entries_refine_earlier_patterns_and_others_are_untouched() {
        let mut m = bare("claude-haiku-4-5");
        apply(ProviderKind::Anthropic, "anthropic", &mut m);
        let p = m.pricing.unwrap();
        assert!(
            (p.input_per_mtok - 1.0).abs() < 1e-9,
            "haiku's own price wins"
        );
        let mut other = bare("some/other-model");
        other.context_window = Some(42);
        apply(ProviderKind::OpenAiChat, "openrouter", &mut other);
        assert_eq!(other.context_window, Some(42));
        assert!(other.pricing.is_none());
    }
}
