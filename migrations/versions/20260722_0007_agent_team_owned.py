"""team-owned agent libraries

Each team has its own agent library instead of sharing a project-wide pool.
Adds agent_profiles.team_id (CASCADE on team delete), backfills it from the
existing team_members wiring, and swaps the (project_id, name) uniqueness for
per-team uniqueness (with a project-level rule for unassigned/draft agents).

Revision ID: 0007
Revises: 0006
Create Date: 2026-07-22
"""

from __future__ import annotations

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "0007"
down_revision: str | None = "0006"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column(
        "agent_profiles",
        sa.Column("team_id", sa.dialects.postgresql.UUID(as_uuid=True), nullable=True),
    )
    op.create_foreign_key(
        "fk_agent_profiles_team",
        "agent_profiles",
        "teams",
        ["team_id"],
        ["id"],
        ondelete="CASCADE",
    )
    # Backfill: an agent used by a team becomes owned by it. A profile shared by
    # more than one team is assigned to the earliest team that uses it (rare;
    # shared agents aren't auto-split into copies).
    op.execute(
        sa.text(
            """
            UPDATE agent_profiles AS a
            SET team_id = tm.team_id
            FROM (
                SELECT DISTINCT ON (profile_id) profile_id, team_id
                FROM team_members
                ORDER BY profile_id, created_at, team_id
            ) AS tm
            WHERE a.id = tm.profile_id
            """
        )
    )
    # Swap project-wide name uniqueness for per-team (owned) / per-project (draft).
    op.drop_constraint("uq_agent_profiles_project_name", "agent_profiles", type_="unique")
    op.create_index(
        "uq_agent_profiles_team_name",
        "agent_profiles",
        ["team_id", "name"],
        unique=True,
        postgresql_where=sa.text("team_id IS NOT NULL"),
    )
    op.create_index(
        "uq_agent_profiles_project_name_draft",
        "agent_profiles",
        ["project_id", "name"],
        unique=True,
        postgresql_where=sa.text("team_id IS NULL"),
    )
    op.create_index("ix_agent_profiles_team", "agent_profiles", ["team_id"])


def downgrade() -> None:
    op.drop_index("ix_agent_profiles_team", table_name="agent_profiles")
    op.drop_index("uq_agent_profiles_project_name_draft", table_name="agent_profiles")
    op.drop_index("uq_agent_profiles_team_name", table_name="agent_profiles")
    op.create_unique_constraint(
        "uq_agent_profiles_project_name", "agent_profiles", ["project_id", "name"]
    )
    op.drop_constraint("fk_agent_profiles_team", "agent_profiles", type_="foreignkey")
    op.drop_column("agent_profiles", "team_id")
