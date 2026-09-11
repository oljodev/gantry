//! Core types shared by every Gantry crate and, through `specta`, by the frontend.
//!
//! This crate holds only data: id newtypes, the message model, agent events, settings, the
//! chat DTOs and errors. It performs no IO. See `docs/plan/01-architecture-overview.md` §2.

#![forbid(unsafe_code)]

mod app_info;
pub mod artifact;
pub mod attachment;
pub mod chat;
pub mod command;
pub mod connector;
pub mod error;
pub mod event;
pub mod file;
pub mod grant;
pub mod guardrail;
pub mod ids;
pub mod interaction;
pub mod judge;
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
pub use chat::{
    ChatDetail, ChatSummary, Feedback, SearchHit, SearchHitKind, Surface, TurnDto, TurnStatus,
};
pub use command::{CommandClass, classify};
pub use connector::{
    AuthState, AuthType, CatalogEntryDto, ConnectorConfig, ConnectorInstanceDto, ConnectorKind,
    RuntimeRequirement, ServerInfo, ToolInfo,
};
pub use error::{ErrorDto, GantryError, ProviderErrorKind};
pub use event::{AgentEvent, AgentEventBatch, AgentEventKind, TurnSnapshot};
pub use file::{EditOp, FileEditDto};
pub use grant::{ArgScope, ChatGrant, GrantScope, GrantSource};
pub use guardrail::{
    GuardrailHit, GuardrailKind, GuardrailRule, GuardrailSettings, GuardrailVerdict, Guardrails,
};
pub use ids::{
    ArtifactId, CallId, ChatId, EventId, GrantId, InstanceId, InteractionId, MessageId, ProjectId,
    TurnId,
};
pub use interaction::{
    AccessDecision, AccessRequest, ConnectorSuggestion, Interaction, InteractionKind,
    InteractionPayload, InteractionResolution, InteractionStatus, PermissionDecision,
    PermissionRequest, SuggestionOutcome,
};
pub use judge::{JudgeDecision, JudgeFlag, JudgeSource, JudgeVerdict};
pub use message::{
    ContentPart, MediaSource, Message, ProviderKind, ResultPart, Role, StopReason, Usage,
};
pub use settings::{
    AdvancedSettings, AppearanceSettings, ChatSettings, Density, MediaOptions, Mode, ModelRef,
    ProviderId, ReasoningEffort, Settings, SettingsPatch, Theme,
};
pub use time::now_ms;
pub use tool::{
    DecisionSource, PlanModePolicy, RiskTier, ToolCallDto, ToolCallStatus, ToolDef, ToolDisplay,
    ToolDisplayKind, ToolStream, result_preview,
};
