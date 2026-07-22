"""Pydantic wire schemas for the control plane API.

``TaskOut`` / ``TaskEventOut`` are projections of the ORM rows (the API never
exposes ORM objects). The WebSocket protocol is a stream of the same shapes
wrapped in typed messages: ``{"type": "task" | "event", "data": ...}``.
"""

from __future__ import annotations

import uuid
from datetime import datetime
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator

from gantry.core.models import EventType, ProviderType, TaskKind, TaskStatus


class TaskCreateRequest(BaseModel):
    goal: str = Field(min_length=1)
    kind: TaskKind = TaskKind.EXECUTE
    #: Repository the worker clones and delivers a branch to (omit for repo-less tasks).
    repo_url: str | None = None
    base_branch: str | None = None
    #: Configured provider to run on (vault-backed API key + base_url).
    provider_id: uuid.UUID | None = None
    #: Model name — a full LiteLLM string, or a bare name when provider_id is
    #: set (the server prefixes it). Defaults to the provider's/server's model.
    model: str | None = None
    max_steps: int | None = Field(default=None, ge=1)
    priority: int = 0
    max_attempts: int = Field(default=3, ge=1)
    #: Explicit skill names to inject (auto-matching by goal happens anyway).
    skills: list[str] | None = None
    #: Extra payload fields passed through to the agent verbatim.
    payload: dict[str, Any] = Field(default_factory=dict)

    def build_payload(self) -> dict[str, Any]:
        merged = dict(self.payload)
        merged["goal"] = self.goal
        for key in ("repo_url", "base_branch", "model", "max_steps", "skills"):
            value = getattr(self, key)
            if value is not None:
                merged[key] = value
        if self.provider_id is not None:
            merged["provider_id"] = str(self.provider_id)
        return merged


class TaskOut(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: uuid.UUID
    workspace_id: uuid.UUID
    parent_task_id: uuid.UUID | None
    root_task_id: uuid.UUID
    kind: TaskKind
    status: TaskStatus
    priority: int
    payload: dict[str, Any]
    result: dict[str, Any] | None
    last_error: str | None
    attempt: int
    max_attempts: int
    claimed_by: str | None
    cancel_requested: bool
    scheduled_at: datetime
    created_at: datetime
    updated_at: datetime


class TaskEventOut(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: int
    task_id: uuid.UUID
    seq: int
    event_type: EventType
    payload: dict[str, Any]
    created_at: datetime


class TaskListResponse(BaseModel):
    tasks: list[TaskOut]


class TaskEventsResponse(BaseModel):
    events: list[TaskEventOut]


class StatsResponse(BaseModel):
    """Fleet-level counters for the dashboard header."""

    total: int
    statuses: dict[str, int]
    active_workers: int
    recent_workers: int
    prompt_tokens: int
    completion_tokens: int
    events_last_hour: int


class SkillOut(BaseModel):
    name: str
    description: str
    match: list[str]


class SkillsResponse(BaseModel):
    skills: list[SkillOut]


class ApprovalResolveRequest(BaseModel):
    decision: Literal["approved", "rejected"]
    comment: str = ""
    resolved_by: str = "operator"


class ApprovalItem(BaseModel):
    """One inbox entry: the waiting task plus its approval_requested event."""

    task: TaskOut
    request: TaskEventOut


class ApprovalsResponse(BaseModel):
    approvals: list[ApprovalItem]


class QuestionAnswerRequest(BaseModel):
    answer: str = Field(min_length=1)
    resolved_by: str = "operator"


class QuestionItem(BaseModel):
    """One inbox entry: the waiting task plus its ask_user_question event."""

    task: TaskOut
    request: TaskEventOut


class QuestionsResponse(BaseModel):
    questions: list[QuestionItem]


class TaskMessage(BaseModel):
    """WS: a task snapshot — sent on connect and after status transitions."""

    type: Literal["task"] = "task"
    data: TaskOut


class EventMessage(BaseModel):
    """WS: one event from the task's append-only log."""

    type: Literal["event"] = "event"
    data: TaskEventOut


class ProviderCreateRequest(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    provider_type: ProviderType
    #: Write-only: encrypted into the vault, never returned by any endpoint.
    api_key: str | None = None
    base_url: str | None = None
    default_model: str = Field(min_length=1, max_length=200)

    @model_validator(mode="after")
    def _local_requires_base_url(self) -> ProviderCreateRequest:
        if self.provider_type is ProviderType.LOCAL and not self.base_url:
            raise ValueError("base_url is required for local providers")
        return self


class ProviderOut(BaseModel):
    """Redaction by construction: there is no api_key field to leak."""

    model_config = ConfigDict(from_attributes=True)

    id: uuid.UUID
    name: str
    provider_type: ProviderType
    base_url: str | None
    default_model: str
    api_key_last4: str
    created_at: datetime


class ProvidersResponse(BaseModel):
    providers: list[ProviderOut]


class ProviderTestResponse(BaseModel):
    ok: bool
    model: str
    error: str | None = None


class GithubTokenRequest(BaseModel):
    token: str = Field(min_length=1)


class GithubStatusResponse(BaseModel):
    connected: bool
    login: str | None = None
    last4: str | None = None


class GithubRepo(BaseModel):
    full_name: str
    private: bool
    default_branch: str
    clone_url: str
    pushed_at: datetime | None = None


class GithubReposResponse(BaseModel):
    repos: list[GithubRepo]


class AgentProfileIn(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    role: str = ""
    system_prompt: str | None = None
    provider_id: uuid.UUID | None = None
    #: Bare model name (prefixed via the provider) or a full LiteLLM string.
    model: str | None = None
    max_steps: int | None = Field(default=None, ge=1)
    can_spawn: bool = False
    gated_tools: list[str] = Field(default_factory=list)
    skills: list[str] = Field(default_factory=list)


class AgentProfileOut(AgentProfileIn):
    model_config = ConfigDict(from_attributes=True)

    id: uuid.UUID
    created_at: datetime
    updated_at: datetime


class AgentsResponse(BaseModel):
    agents: list[AgentProfileOut]


class TeamNodeIn(BaseModel):
    profile_id: uuid.UUID
    children: list[TeamNodeIn] = Field(default_factory=list)


class TeamWriteRequest(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    description: str = ""
    root: TeamNodeIn


class TeamNodeOut(BaseModel):
    profile: AgentProfileOut
    children: list[TeamNodeOut] = Field(default_factory=list)


class TeamOut(BaseModel):
    id: uuid.UUID
    name: str
    description: str
    root: TeamNodeOut


class TeamSummary(BaseModel):
    id: uuid.UUID
    name: str
    description: str
    member_count: int


class TeamsResponse(BaseModel):
    teams: list[TeamSummary]


class TeamLaunchRequest(BaseModel):
    goal: str = Field(min_length=1)
    repo_url: str | None = None
    base_branch: str | None = None
    priority: int = 0


TeamNodeIn.model_rebuild()
TeamNodeOut.model_rebuild()
