//! The model provider layer (docs/plan/02): one internal request/stream model, one client per
//! wire API. M1 ships the OpenAI-compatible Chat Completions client with the OpenRouter profile;
//! Anthropic, OpenAI Responses and Gemini arrive with M4.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod error;
pub mod openai_chat;
pub mod provider;
pub mod registry;
pub mod retry;
pub mod sse;
pub mod tools;

pub use error::ProviderError;
pub use openai_chat::{CompatProfile, OpenAiChatProvider};
pub use provider::{
    CacheSupport, ChatRequest, ChatStream, KeyInfo, ModelCapabilities, ModelInfo, Pricing,
    Provider, ReasoningSupport, RequestMetadata, StreamEvent, ToolChoice, ToolSpec,
};
pub use registry::ProviderRegistry;
pub use tools::{ToolNameMap, ToolSchemaSanitizer, model_tool_name};

/// The plan document that specifies this crate.
pub const PLAN_DOCUMENT: &str = "docs/plan/02-model-providers.md";
