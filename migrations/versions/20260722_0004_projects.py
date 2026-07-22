"""projects: group runs, agents, and teams into top-level containers

Revision ID: 0004
Revises: 0003
Create Date: 2026-07-22

Adds a ``projects`` table and scopes ``tasks``/``agent_profiles``/``teams`` to
it. Every pre-existing row is backfilled into a seeded "Default" project so the
migration is non-destructive.
"""

from __future__ import annotations

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "0004"
down_revision: str | None = "0003"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None

_WORKSPACE = "00000000-0000-0000-0000-000000000001"
_PROJECT = "00000000-0000-0000-0000-000000000002"
_SCOPED = ("tasks", "agent_profiles", "teams")


def upgrade() -> None:
    op.create_table(
        "projects",
        sa.Column("id", sa.dialects.postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column("workspace_id", sa.dialects.postgresql.UUID(as_uuid=True), nullable=False),
        sa.Column("name", sa.String(length=100), nullable=False),
        sa.Column("description", sa.Text(), nullable=False, server_default=""),
        sa.Column("default_repo_url", sa.Text(), nullable=True),
        sa.Column("default_base_branch", sa.String(length=200), nullable=True),
        sa.Column(
            "created_at", sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False
        ),
        sa.Column(
            "updated_at", sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False
        ),
        sa.UniqueConstraint("workspace_id", "name", name="uq_projects_workspace_name"),
    )
    op.create_index("ix_projects_workspace", "projects", ["workspace_id"])

    # Seed the Default project every existing row is backfilled into.
    op.execute(
        sa.text(
            "INSERT INTO projects (id, workspace_id, name, description) "
            "VALUES (CAST(:pid AS uuid), CAST(:wid AS uuid), 'Default', 'Default project')"
        ).bindparams(pid=_PROJECT, wid=_WORKSPACE)
    )

    for table in _SCOPED:
        op.add_column(
            table, sa.Column("project_id", sa.dialects.postgresql.UUID(as_uuid=True), nullable=True)
        )
        op.execute(
            sa.text(f"UPDATE {table} SET project_id = CAST(:pid AS uuid)").bindparams(pid=_PROJECT)
        )
        op.alter_column(table, "project_id", nullable=False)
        op.create_foreign_key(
            f"fk_{table}_project", table, "projects", ["project_id"], ["id"], ondelete="RESTRICT"
        )
        op.create_index(f"ix_{table}_project", table, ["project_id"])

    # Names become unique per project rather than per workspace.
    op.drop_constraint("uq_agent_profiles_workspace_name", "agent_profiles", type_="unique")
    op.create_unique_constraint(
        "uq_agent_profiles_project_name", "agent_profiles", ["project_id", "name"]
    )
    op.drop_constraint("uq_teams_workspace_name", "teams", type_="unique")
    op.create_unique_constraint("uq_teams_project_name", "teams", ["project_id", "name"])


def downgrade() -> None:
    op.drop_constraint("uq_teams_project_name", "teams", type_="unique")
    op.create_unique_constraint("uq_teams_workspace_name", "teams", ["workspace_id", "name"])
    op.drop_constraint("uq_agent_profiles_project_name", "agent_profiles", type_="unique")
    op.create_unique_constraint(
        "uq_agent_profiles_workspace_name", "agent_profiles", ["workspace_id", "name"]
    )
    for table in _SCOPED:
        op.drop_index(f"ix_{table}_project", table_name=table)
        op.drop_constraint(f"fk_{table}_project", table, type_="foreignkey")
        op.drop_column(table, "project_id")
    op.drop_index("ix_projects_workspace", table_name="projects")
    op.drop_table("projects")
