//! Core types shared by every Gantry crate and, through `specta`, by the frontend.
//!
//! This crate holds only data: id newtypes, the message model, agent events, settings, the
//! chat DTOs and errors. It performs no IO. See `docs/plan/01-architecture-overview.md` §2.

#![forbid(unsafe_code)]

mod app_info;
pub mod chat;
pub mod error;
pub mod event;
pub mod ids;
pub mod message;
pub mod settings;
pub mod time;

pub use app_info::AppInfo;
pub use chat::{ChatDetail, ChatSummary, Feedback, TurnDto, TurnStatus};
pub use error::{ErrorDto, GantryError, ProviderErrorKind};
pub use event::{AgentEvent, AgentEventBatch, AgentEventKind, TurnSnapshot};
pub use ids::{ArtifactId, CallId, ChatId, EventId, InstanceId, MessageId, ProjectId, TurnId};
pub use message::{
    ContentPart, MediaSource, Message, ProviderKind, ResultPart, Role, StopReason, Usage,
};
pub use settings::{
    AdvancedSettings, AppearanceSettings, ChatSettings, Density, Mode, ModelRef, ProviderId,
    ReasoningEffort, Settings, SettingsPatch, Theme,
};
pub use time::now_ms;
