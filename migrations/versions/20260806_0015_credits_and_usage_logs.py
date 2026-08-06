"""users + gantry credits balance, per-call llm usage log, task ownership

Revision ID: 0015
Revises: 0014
Create Date: 2026-08-06
"""

from __future__ import annotations

import sqlalchemy as sa
from alembic import op
from sqlalchemy.dialects.postgresql import UUID

revision: str = "0015"
down_revision: str | None = "0014"
branch_labels: str | None = None
depends_on: str | None = None

#: Mirrors gantry.core.models.LOCAL_USER_ID / DEFAULT_WORKSPACE_ID. Spelled out
#: rather than imported so the migration keeps meaning what it meant on the day
#: it ran, even if the constants later move.
LOCAL_USER_ID = "00000000-0000-0000-0000-000000000003"
DEFAULT_WORKSPACE_ID = "00000000-0000-0000-0000-000000000001"


def upgrade() -> None:
    op.create_table(
        "users",
        sa.Column("id", UUID(as_uuid=True), primary_key=True),
        sa.Column("workspace_id", UUID(as_uuid=True), nullable=False),
        sa.Column("email", sa.String(320), nullable=False),
        sa.Column("subject", sa.String(128), nullable=False, server_default=""),
        sa.Column(
            "gantry_credits_balance",
            sa.Numeric(18, 6),
            nullable=False,
            server_default=sa.text("0"),
        ),
        sa.Column(
            "created_at", sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
        ),
        sa.Column(
            "updated_at", sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
        ),
        sa.UniqueConstraint("workspace_id", "email", name="uq_users_workspace_email"),
    )

    # The account every run bills to when auth is off (local dev, CI). Seeded
    # with a starting balance so a fresh install can actually run something —
    # without it the very first local task would be refused for lack of credit.
    op.execute(
        sa.text(
            """
            INSERT INTO users (id, workspace_id, email, subject, gantry_credits_balance)
            VALUES (CAST(:id AS uuid), CAST(:ws AS uuid), 'local@gantry.local', '', 1000)
            ON CONFLICT DO NOTHING
            """
        ).bindparams(id=LOCAL_USER_ID, ws=DEFAULT_WORKSPACE_ID)
    )

    op.create_table(
        "llm_usage_logs",
        sa.Column("id", UUID(as_uuid=True), primary_key=True),
        sa.Column("workspace_id", UUID(as_uuid=True), nullable=False),
        sa.Column(
            "user_id",
            UUID(as_uuid=True),
            sa.ForeignKey("users.id", ondelete="SET NULL"),
            nullable=True,
        ),
        sa.Column("run_id", UUID(as_uuid=True), nullable=True),
        sa.Column("task_id", UUID(as_uuid=True), nullable=True),
        sa.Column("model_slug", sa.String(200), nullable=False, server_default=""),
        sa.Column("prompt_tokens", sa.BigInteger(), nullable=False, server_default=sa.text("0")),
        sa.Column(
            "completion_tokens", sa.BigInteger(), nullable=False, server_default=sa.text("0")
        ),
        sa.Column("total_tokens", sa.BigInteger(), nullable=False, server_default=sa.text("0")),
        sa.Column(
            "cache_read_tokens", sa.BigInteger(), nullable=False, server_default=sa.text("0")
        ),
        sa.Column(
            "cache_write_tokens", sa.BigInteger(), nullable=False, server_default=sa.text("0")
        ),
        sa.Column("raw_cost_usd", sa.Numeric(18, 8), nullable=False, server_default=sa.text("0")),
        sa.Column(
            "credits_deducted", sa.Numeric(18, 6), nullable=False, server_default=sa.text("0")
        ),
        sa.Column(
            "created_at", sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
        ),
    )
    op.create_index("ix_llm_usage_logs_user", "llm_usage_logs", ["user_id", "created_at"])
    op.create_index("ix_llm_usage_logs_run", "llm_usage_logs", ["run_id"])
    op.create_index("ix_llm_usage_logs_task", "llm_usage_logs", ["task_id"])
    op.create_index("ix_llm_usage_logs_workspace", "llm_usage_logs", ["workspace_id", "created_at"])

    op.add_column(
        "tasks",
        sa.Column(
            "user_id",
            UUID(as_uuid=True),
            sa.ForeignKey("users.id", ondelete="SET NULL"),
            nullable=True,
        ),
    )
    op.create_index("ix_tasks_user", "tasks", ["user_id"])


def downgrade() -> None:
    op.drop_index("ix_tasks_user", table_name="tasks")
    op.drop_column("tasks", "user_id")
    op.drop_index("ix_llm_usage_logs_workspace", table_name="llm_usage_logs")
    op.drop_index("ix_llm_usage_logs_task", table_name="llm_usage_logs")
    op.drop_index("ix_llm_usage_logs_run", table_name="llm_usage_logs")
    op.drop_index("ix_llm_usage_logs_user", table_name="llm_usage_logs")
    op.drop_table("llm_usage_logs")
    op.drop_table("users")
