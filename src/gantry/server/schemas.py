"""Pydantic wire schemas for the control plane API.

``TaskOut`` / ``TaskEventOut`` are projections of the ORM rows (the API never
exposes ORM objects). The WebSocket protocol is a stream of the same shapes
wrapped in typed messages: ``{"type": "task" | "event", "data": ...}``.
"""

from __future__ import annotations

import uuid
from datetime import datetime
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field

from gantry.core.models import EventType, TaskKind, TaskStatus


class TaskCreateRequest(BaseModel):
    goal: str = Field(min_length=1)
    kind: TaskKind = TaskKind.EXECUTE
    #: Repository the worker clones and delivers a branch to (omit for repo-less tasks).
    repo_url: str | None = None
    base_branch: str | None = None
    #: LiteLLM model string; defaults to the server's configured model.
    model: str | None = None
    max_steps: int | None = Field(default=None, ge=1)
    priority: int = 0
    max_attempts: int = Field(default=3, ge=1)
    #: Extra payload fields passed through to the agent verbatim.
    payload: dict[str, Any] = Field(default_factory=dict)

    def build_payload(self) -> dict[str, Any]:
        merged = dict(self.payload)
        merged["goal"] = self.goal
        for key in ("repo_url", "base_branch", "model", "max_steps"):
            value = getattr(self, key)
            if value is not None:
                merged[key] = value
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


class TaskMessage(BaseModel):
    """WS: a task snapshot — sent on connect and after status transitions."""

    type: Literal["task"] = "task"
    data: TaskOut


class EventMessage(BaseModel):
    """WS: one event from the task's append-only log."""

    type: Literal["event"] = "event"
    data: TaskEventOut
