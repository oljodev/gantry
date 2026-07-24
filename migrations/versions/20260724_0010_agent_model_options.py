"""per-leader model menu on agent profiles

Revision ID: 0010
Revises: 0009
Create Date: 2026-07-24
"""

from __future__ import annotations

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op
from sqlalchemy.dialects.postgresql import JSONB

revision: str = "0010"
down_revision: str | None = "0009"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    # A curated menu of models (slug + when-to-use note) an Autonomous Leader may
    # assign to the workers it spawns. Snapshotted into the launch payload and
    # injected into the leader prompt; keys never appear here (only model slugs).
    op.add_column(
        "agent_profiles",
        sa.Column(
            "model_options",
            JSONB(),
            nullable=False,
            server_default=sa.text("'[]'::jsonb"),
        ),
    )


def downgrade() -> None:
    op.drop_column("agent_profiles", "model_options")
