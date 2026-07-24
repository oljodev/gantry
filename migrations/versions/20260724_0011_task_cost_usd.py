"""per-task cost_usd for the run budget ledger

Revision ID: 0011
Revises: 0010
Create Date: 2026-07-24
"""

from __future__ import annotations

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "0011"
down_revision: str | None = "0010"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    # Estimated USD a task spent, set once on its terminal transition. Run spend
    # is SUM(cost_usd) over a root_task_id — the per-run budget brake reads it.
    op.add_column(
        "tasks",
        sa.Column("cost_usd", sa.Float(), nullable=False, server_default=sa.text("0")),
    )


def downgrade() -> None:
    op.drop_column("tasks", "cost_usd")
