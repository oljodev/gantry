"""cooperative cancellation flag on tasks

Revision ID: 0003
Revises: 0002
Create Date: 2026-07-22
"""

from __future__ import annotations

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "0003"
down_revision: str | None = "0002"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    # Set by the cancel endpoint on a live task; the owning worker reads it at
    # its next heartbeat/step boundary and aborts cooperatively.
    op.add_column(
        "tasks",
        sa.Column(
            "cancel_requested",
            sa.Boolean(),
            nullable=False,
            server_default=sa.false(),
        ),
    )


def downgrade() -> None:
    op.drop_column("tasks", "cancel_requested")
