//! Core types shared by every Gantry crate and, through `specta`, by the frontend.
//!
//! This crate holds only data: id newtypes, the message model, agent events, settings, the
//! chat DTOs and errors. It performs no IO. See `docs/plan/01-architecture-overview.md` §2.

#![forbid(unsafe_code)]

mod app_info;
pub mod artifact;
pub mod attachment;
pub mod chat;
pub mod error;
pub mod event;
pub mod ids;
pub mod interaction;
pub mod message;
pub mod settings;
pub mod time;
pub mod tool;

pub use app_info::AppInfo;
pub use artifact::{
    ArtifactContent, ArtifactDto, ArtifactVersionDto, RenderError, RenderReport, RenderStatus,
    VersionSource,
};
pub use attachment::{AttachmentInput, MAX_IMAGE_BYTES, MAX_TEXT_BYTES};
pub use chat::{ChatDetail, ChatSummary, Feedback, SearchHit, SearchHitKind, TurnDto, TurnStatus};
pub use error::{ErrorDto, GantryError, ProviderErrorKind};
pub use event::{AgentEvent, AgentEventBatch, AgentEventKind, TurnSnapshot};
pub use ids::{
    ArtifactId, CallId, ChatId, EventId, InstanceId, InteractionId, MessageId, ProjectId, TurnId,
};
pub use interaction::{
    Interaction, InteractionKind, InteractionPayload, InteractionResolution, InteractionStatus,
    PermissionDecision, PermissionRequest,
};
pub use message::{
    ContentPart, MediaSource, Message, ProviderKind, ResultPart, Role, StopReason, Usage,
};
pub use settings::{
    AdvancedSettings, AppearanceSettings, ChatSettings, Density, Mode, ModelRef, ProviderId,
    ReasoningEffort, Settings, SettingsPatch, Theme,
};
pub use time::now_ms;
pub use tool::{
    DecisionSource, PlanModePolicy, RiskTier, ToolCallDto, ToolCallStatus, ToolDef, ToolDisplay,
    ToolDisplayKind, result_preview,
};
