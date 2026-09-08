//! The `Provider` trait and the internal request, stream and model types (docs/plan/02 §2).

use std::pin::Pin;

use async_trait::async_trait;
use futures_util::Stream;
use gantry_core::{
    CallId, ChatId, ContentPart, Message, ProviderId, ProviderKind, ReasoningEffort, StopReason,
    TurnId, Usage,
};
use serde::{Deserialize, Serialize};

use crate::error::ProviderError;

pub type ChatStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, ProviderError>> + Send>>;

/// One request to a model. Every request is streamed; non-streaming callers collect.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    /// The frozen system prompt text (10 §2).
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub tool_choice: ToolChoice,
    pub max_output_tokens: u32,
    pub reasoning: ReasoningEffort,
    /// Tools the provider runs on its own servers (02 §3); ignored where none exist.
    pub server_tools: Vec<ServerTool>,
    pub metadata: RequestMetadata,
    /// Merged into the wire request last; an escape hatch, empty by default.
    pub provider_options: serde_json::Value,
}

impl ChatRequest {
    /// A text-only request with default settings.
    #[must_use]
    pub fn new(
        model: impl Into<String>,
        system: impl Into<String>,
        messages: Vec<Message>,
    ) -> Self {
        Self {
            model: model.into(),
            system: system.into(),
            messages,
            tools: Vec::new(),
            tool_choice: ToolChoice::Auto,
            max_output_tokens: 8192,
            reasoning: ReasoningEffort::Off,
            server_tools: Vec::new(),
            metadata: RequestMetadata::default(),
            provider_options: serde_json::Value::Null,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RequestMetadata {
    pub chat_id: Option<ChatId>,
    pub turn_id: Option<TurnId>,
}

/// A tool the provider executes itself and reports as opaque blocks (02 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerTool {
    WebSearch { max_uses: Option<u32> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Named(String),
}

/// A tool as the model sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    /// Model-facing, e.g. `filesystem__read_file`.
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub strict: bool,
    pub deferred: bool,
    pub stream_args: bool,
}

/// What a client yields while a message streams.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    MessageStart {
        provider_message_id: Option<String>,
    },
    TextDelta {
        index: u32,
        text: String,
    },
    ThinkingDelta {
        index: u32,
        text: String,
    },
    ThinkingSignature {
        index: u32,
        signature: String,
    },
    ToolCallStart {
        index: u32,
        id: CallId,
        name: String,
    },
    ToolCallArgsDelta {
        index: u32,
        json_fragment: String,
    },
    ToolCallEnd {
        index: u32,
        args: serde_json::Value,
    },
    /// Opaque blocks to persist and replay verbatim.
    ProviderBlock {
        index: u32,
        part: ContentPart,
    },
    Usage(Usage),
    MessageEnd {
        stop_reason: StopReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReasoningSupport {
    None,
    /// Accepts an effort level.
    Effort,
    /// Accepts a token budget.
    Budget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CacheSupport {
    None,
    Automatic,
    Explicit,
}

/// What a model takes in and gives back. `File` covers PDFs and documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Text,
    Image,
    Audio,
    Video,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ModelCapabilities {
    /// What the model accepts and what it produces. A model that produces something other than
    /// text is a different kind of thing to talk to, which is what the picker sorts by; `vision`
    /// and `pdf_input` below are the two input cases the composer already asks about by name.
    #[serde(default)]
    pub input: Vec<Modality>,
    #[serde(default)]
    pub output: Vec<Modality>,
    pub tools: bool,
    pub parallel_tools: bool,
    pub streams_tool_args: bool,
    pub vision: bool,
    pub pdf_input: bool,
    pub reasoning: ReasoningSupport,
    pub server_web_search: bool,
    pub structured_output: bool,
    pub prompt_caching: CacheSupport,
}

impl Default for ModelCapabilities {
    /// Conservative defaults for a model nobody described: tools on, everything else off.
    fn default() -> Self {
        Self {
            input: vec![Modality::Text],
            output: vec![Modality::Text],
            tools: true,
            parallel_tools: false,
            streams_tool_args: false,
            vision: false,
            pdf_input: false,
            reasoning: ReasoningSupport::None,
            server_web_search: false,
            structured_output: false,
            prompt_caching: CacheSupport::None,
        }
    }
}

/// US dollars per million tokens, plus the per-unit prices some models carry instead.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Pricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: Option<f64>,
    /// Dollars for one image sent to the model.
    #[serde(default)]
    pub image_input_usd: Option<f64>,
    /// Dollars for one image the model produces, where it reports one.
    #[serde(default)]
    pub image_output_usd: Option<f64>,
    /// Dollars per call, for models priced by the request rather than by the token.
    #[serde(default)]
    pub request_usd: Option<f64>,
}

/// One row of a provider's model list, as the UI and the catalog cache see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    /// When the model was released, in Unix seconds, where the provider says. A year-old model
    /// and a week-old one are different tools, and the list gives no other way to tell them apart.
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub created_at: Option<i64>,
    pub context_window: Option<u32>,
    pub max_output: Option<u32>,
    pub pricing: Option<Pricing>,
    pub capabilities: ModelCapabilities,
}

/// What a key check returns (OpenRouter's `GET /key`; other providers report less).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, specta::Type)]
pub struct KeyInfo {
    pub label: Option<String>,
    pub limit_usd: Option<f64>,
    pub limit_remaining_usd: Option<f64>,
    pub usage_usd: Option<f64>,
    pub is_free_tier: Option<bool>,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &ProviderId;
    fn kind(&self) -> ProviderKind;
    /// Whether a key is configured; without one every network call fails with `Auth`.
    fn has_key(&self) -> bool;
    /// What the provider knows about a model from its last model list; `None` when unknown.
    fn model_info(&self, model: &str) -> Option<ModelInfo> {
        let _ = model;
        None
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    /// Verifies the key with the cheapest call the provider offers.
    async fn check_key(&self) -> Result<KeyInfo, ProviderError>;
    async fn stream(&self, req: ChatRequest) -> Result<ChatStream, ProviderError>;
}
