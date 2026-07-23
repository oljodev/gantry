"""saved co-pilot chats

A co-pilot conversation can be saved, reopened, and continued. The transcript
lives in the referenced tasks' event logs; a session only stores which tasks
(and the user's messages) made up the chat.

Revision ID: 0008
Revises: 0007
Create Date: 2026-07-22
"""

from __future__ import annotations

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op
from sqlalchemy.dialects import postgresql

revision: str = "0008"
down_revision: str | None = "0007"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.create_table(
        "copilot_sessions",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column("workspace_id", postgresql.UUID(as_uuid=True), nullable=False),
        sa.Column(
            "project_id",
            postgresql.UUID(as_uuid=True),
            sa.ForeignKey("projects.id", ondelete="CASCADE"),
            nullable=False,
        ),
        sa.Column("kind", sa.String(20), nullable=False),
        sa.Column("team_id", postgresql.UUID(as_uuid=True), nullable=True),
        sa.Column("title", sa.Text(), nullable=False, server_default=""),
        sa.Column(
            "turns",
            postgresql.JSONB(),
            nullable=False,
            server_default=sa.text("'[]'::jsonb"),
        ),
        sa.Column(
            "created_at", sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
        ),
        sa.Column(
            "updated_at", sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
        ),
    )
    op.create_index("ix_copilot_sessions_project", "copilot_sessions", ["project_id"])
    op.create_index("ix_copilot_sessions_team", "copilot_sessions", ["team_id"])


def downgrade() -> None:
    op.drop_index("ix_copilot_sessions_team", table_name="copilot_sessions")
    op.drop_index("ix_copilot_sessions_project", table_name="copilot_sessions")
    op.drop_table("copilot_sessions")
