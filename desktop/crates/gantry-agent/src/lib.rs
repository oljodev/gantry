//! The agent: chats, turns, events and the system prompt (docs/plan/01 §3, 05 §3, 10). M1 runs
//! text-only turns against one provider; tools, permissions, the judge and interactions arrive
//! with M3 and later.

#![forbid(unsafe_code)]

pub mod chats;
pub mod events;
pub mod runner;
pub mod system_prompt;
pub mod turn_manager;

pub use chats::{ChatBook, ChatPatch};
pub use events::{Batcher, EventSink, FanoutSink};
pub use system_prompt::{CORE_VERSION, PromptContext, SystemPromptBuilder};
pub use turn_manager::{ProviderSource, TurnManager};

/// The plan document that specifies this crate.
pub const PLAN_DOCUMENT: &str = "docs/plan/01-architecture-overview.md";
