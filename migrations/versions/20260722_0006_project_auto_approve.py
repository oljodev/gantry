"""per-project HITL auto-accept flag

Revision ID: 0006
Revises: 0005
Create Date: 2026-07-22
"""

from __future__ import annotations

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "0006"
down_revision: str | None = "0005"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    # When on, gated tool calls in this project's runs are auto-approved
    # (still recorded in the trace via approval_requested/resolved).
    op.add_column(
        "projects",
        sa.Column("auto_approve", sa.Boolean(), nullable=False, server_default=sa.false()),
    )


def downgrade() -> None:
    op.drop_column("projects", "auto_approve")
