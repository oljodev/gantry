//! The agent: chats, turns, the tool loop, permissions, interactions, events and the system
//! prompt (docs/plan/01 §3, 04, 05 §3, 10): the tool round loop with the mode policy, the
//! guardrail floor, standing grants, the guard of 04 §6 and the permission prompts everything
//! else falls back to.

#![forbid(unsafe_code)]

pub mod artifacts;
pub mod attachments;
pub mod chats;
pub mod context;
pub mod events;
pub mod export;
pub mod interactions;
pub mod judge;
pub mod memory;
pub mod permissions;
pub mod persist;
pub mod projects;
pub mod runner;
pub mod runtime_tools;
pub mod skills;
pub mod system_prompt;
pub mod title;
pub mod tools;
pub mod turn_manager;

pub use artifacts::{Artifacts, user_change_note};
pub use chats::{ChatBook, ChatPatch, NewAttachment, NewChat};
pub use events::{Batcher, EventSink, FanoutSink};
pub use export::ExportFormat;
pub use interactions::Interactions;
pub use memory::{Memories, MemoryEdit};
pub use projects::Projects;
pub use runtime_tools::{RuntimeTools, catalog::ConnectorAccess};
pub use skills::Skills;
pub use system_prompt::{
    CORE_VERSION, PromptContext, SystemPromptBuilder, chat_instructions_note, connector_inventory,
    memory_note,
};
pub use tools::ToolSet;
pub use turn_manager::{ChatNotifier, ProviderSource, TurnManager};

/// The plan document that specifies this crate.
pub const PLAN_DOCUMENT: &str = "docs/plan/01-architecture-overview.md";
