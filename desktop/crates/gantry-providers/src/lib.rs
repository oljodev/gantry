//! The model provider layer (docs/plan/02): one internal request/stream model, one client per
//! wire API: `anthropic` (Messages), `openai_responses` (Responses), `openai_chat` (Chat
//! Completions with the `openrouter`, `xai` and `custom` profiles) and `gemini` (Interactions).

#![forbid(unsafe_code)]

pub mod anthropic;
pub mod catalog;
pub mod error;
pub mod gemini;
mod http;
pub mod openai_chat;
pub mod openai_responses;
pub mod overrides;
pub mod provider;
pub mod pump;
pub mod registry;
pub mod retry;
pub mod sse;
pub mod tools;

pub use anthropic::AnthropicProvider;
pub use error::ProviderError;
pub use gemini::GeminiProvider;
pub use http::http_client;
pub use openai_chat::{CompatProfile, OpenAiChatProvider, media::MAX_MEDIA_BYTES};
pub use openai_responses::OpenAiResponsesProvider;
pub use provider::{
    CacheSupport, ChatRequest, ChatStream, KeyInfo, ModelCapabilities, ModelInfo,
    PREFIX_DROP_BLOCK, PREFIX_ERROR, Pricing, Provider, ReasoningSupport, RequestMetadata,
    ServerTool, StreamEvent, ToolChoice, ToolSpec,
};
pub use registry::ProviderRegistry;
pub use tools::{ToolNameMap, ToolSchemaSanitizer, model_tool_name, sanitize_call_id};

/// The plan document that specifies this crate.
pub const PLAN_DOCUMENT: &str = "docs/plan/02-model-providers.md";
