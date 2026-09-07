//! The agent: chats, turns, the tool loop, permissions, interactions, events and the system
//! prompt (docs/plan/01 §3, 04, 05 §3, 10). M3 runs the tool round loop with the mode policy
//! and permission prompts; grants, guardrails, scope and the judge arrive with M6 to M8.

#![forbid(unsafe_code)]

pub mod artifacts;
pub mod attachments;
pub mod chats;
pub mod events;
pub mod export;
pub mod interactions;
pub mod permissions;
pub mod persist;
pub mod runner;
pub mod runtime_tools;
pub mod system_prompt;
pub mod title;
pub mod tools;
pub mod turn_manager;

pub use artifacts::{Artifacts, user_change_note};
pub use chats::{ChatBook, ChatPatch, NewAttachment};
pub use events::{Batcher, EventSink, FanoutSink};
pub use export::ExportFormat;
pub use interactions::Interactions;
pub use runtime_tools::RuntimeTools;
pub use system_prompt::{CORE_VERSION, PromptContext, SystemPromptBuilder};
pub use tools::ToolSet;
pub use turn_manager::{ChatNotifier, ProviderSource, TurnManager};

/// The plan document that specifies this crate.
pub const PLAN_DOCUMENT: &str = "docs/plan/01-architecture-overview.md";
