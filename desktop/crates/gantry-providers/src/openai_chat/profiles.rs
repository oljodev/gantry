//! Per-vendor differences of the Chat Completions dialect (docs/plan/02 §4).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningParam {
    /// `reasoning_effort: "low" | "medium" | "high"` (OpenAI style, xAI).
    OpenAiEffort,
    /// `reasoning: { effort }` (OpenRouter).
    OpenRouterObject,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelsParser {
    /// `{ data: [{ id }] }` and nothing else.
    Plain,
    /// OpenRouter's rich list: context, pricing, supported parameters, modalities.
    OpenRouter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolIdQuirk {
    None,
    /// Some upstreams behind OpenRouter send an empty `id`; synthesize one.
    SynthesizeIfEmpty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCheck {
    /// `GET /key` (OpenRouter): label, limit, usage.
    OpenRouterKey,
    /// Any authenticated call; `GET /models` is the cheapest.
    ListModels,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatProfile {
    pub label: String,
    pub base_url: String,
    pub extra_headers: Vec<(String, String)>,
    pub system_role: &'static str,
    pub reasoning_param: ReasoningParam,
    pub supports_stream_usage: bool,
    pub supports_parallel_flag: bool,
    pub supports_strict: bool,
    pub models_parser: ModelsParser,
    pub tool_id_quirk: ToolIdQuirk,
    pub key_check: KeyCheck,
}

impl CompatProfile {
    #[must_use]
    pub fn openrouter() -> Self {
        Self {
            label: "OpenRouter".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            extra_headers: vec![
                ("HTTP-Referer".into(), "https://oljo.dev".into()),
                ("X-Title".into(), "Gantry".into()),
            ],
            system_role: "system",
            reasoning_param: ReasoningParam::OpenRouterObject,
            // OpenRouter always includes usage in the final chunk; the flag is deprecated there.
            supports_stream_usage: false,
            supports_parallel_flag: true,
            supports_strict: false,
            models_parser: ModelsParser::OpenRouter,
            tool_id_quirk: ToolIdQuirk::SynthesizeIfEmpty,
            key_check: KeyCheck::OpenRouterKey,
        }
    }

    #[must_use]
    pub fn xai() -> Self {
        Self {
            label: "xAI".into(),
            base_url: "https://api.x.ai/v1".into(),
            extra_headers: Vec::new(),
            system_role: "system",
            reasoning_param: ReasoningParam::OpenAiEffort,
            supports_stream_usage: true,
            supports_parallel_flag: true,
            supports_strict: false,
            models_parser: ModelsParser::Plain,
            tool_id_quirk: ToolIdQuirk::None,
            key_check: KeyCheck::ListModels,
        }
    }

    /// A user-supplied OpenAI-compatible endpoint (Ollama, LM Studio, a proxy).
    #[must_use]
    pub fn custom(label: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            base_url: base_url.into(),
            extra_headers: Vec::new(),
            system_role: "system",
            reasoning_param: ReasoningParam::None,
            supports_stream_usage: true,
            supports_parallel_flag: false,
            supports_strict: false,
            models_parser: ModelsParser::Plain,
            tool_id_quirk: ToolIdQuirk::SynthesizeIfEmpty,
            key_check: KeyCheck::ListModels,
        }
    }

    /// The shipped profile for a known provider id, if any.
    #[must_use]
    pub fn for_provider_id(id: &str) -> Option<Self> {
        match id {
            "openrouter" => Some(Self::openrouter()),
            "xai" => Some(Self::xai()),
            _ => None,
        }
    }
}
