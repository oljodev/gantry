"""autonomous-leader flag on agent profiles

Revision ID: 0009
Revises: 0008
Create Date: 2026-07-23
"""

from __future__ import annotations

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "0009"
down_revision: str | None = "0008"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    # When on, the agent runs as a swarm leader: the AUTONOMOUS_LEADER_PROMPT is
    # forced and delegation tools are unlocked (resolved into the launch snapshot).
    op.add_column(
        "agent_profiles",
        sa.Column("autonomous_leader", sa.Boolean(), nullable=False, server_default=sa.false()),
    )


def downgrade() -> None:
    op.drop_column("agent_profiles", "autonomous_leader")
