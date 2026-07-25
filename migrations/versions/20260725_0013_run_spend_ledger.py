"""live run spend ledger + workspace budget ceiling

Revision ID: 0013
Revises: 0012
Create Date: 2026-07-25
"""

from __future__ import annotations

import sqlalchemy as sa
from alembic import op
from sqlalchemy.dialects.postgresql import UUID

revision: str = "0013"
down_revision: str | None = "0012"
branch_labels: str | None = None
depends_on: str | None = None


def upgrade() -> None:
    op.create_table(
        "run_spend",
        sa.Column("root_task_id", UUID(as_uuid=True), primary_key=True),
        sa.Column("workspace_id", UUID(as_uuid=True), nullable=False),
        sa.Column("prompt_tokens", sa.BigInteger(), nullable=False, server_default=sa.text("0")),
        sa.Column(
            "completion_tokens", sa.BigInteger(), nullable=False, server_default=sa.text("0")
        ),
        sa.Column(
            "cache_read_tokens", sa.BigInteger(), nullable=False, server_default=sa.text("0")
        ),
        sa.Column(
            "cache_write_tokens", sa.BigInteger(), nullable=False, server_default=sa.text("0")
        ),
        sa.Column("cost_usd", sa.Float(), nullable=False, server_default=sa.text("0")),
        sa.Column("calls", sa.BigInteger(), nullable=False, server_default=sa.text("0")),
        sa.Column(
            "updated_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.func.now(),
        ),
    )
    op.create_index("ix_run_spend_workspace", "run_spend", ["workspace_id"])
    op.add_column("workspace_controls", sa.Column("budget_usd", sa.Float(), nullable=True))

    # Seed from the settled per-task ledger so an upgrade mid-flight does not
    # reset a running workspace's spend to zero (which would hand every live run
    # a fresh budget). Tasks still in flight contributed 0 there anyway; from
    # here on every call is counted as it happens.
    op.execute(
        """
        INSERT INTO run_spend (root_task_id, workspace_id, cost_usd)
        SELECT root_task_id, MIN(workspace_id::text)::uuid, COALESCE(SUM(cost_usd), 0)
        FROM tasks
        GROUP BY root_task_id
        """
    )


def downgrade() -> None:
    op.drop_column("workspace_controls", "budget_usd")
    op.drop_index("ix_run_spend_workspace", table_name="run_spend")
    op.drop_table("run_spend")
