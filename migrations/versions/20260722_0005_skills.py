"""skills: authored, project-scoped skills (moved off disk)

Revision ID: 0005
Revises: 0004
Create Date: 2026-07-22

Creates the ``skills`` table and seeds the three built-in skills (previously
read-only files under ``skills/``) into the Default project so they remain
available and become editable.
"""

from __future__ import annotations

import json
from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "0005"
down_revision: str | None = "0004"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None

_WORKSPACE = "00000000-0000-0000-0000-000000000001"
_PROJECT = "00000000-0000-0000-0000-000000000002"

_SEEDS = [
    {
        "name": "branch-hygiene",
        "description": "Deliver clean, reviewable branches",
        "match": ["branch", "push", "deliver", "pr", "pull request"],
        "body": (
            "Delivery discipline for your task branch:\n\n"
            "- Deliver exclusively via git_commit_push — never raw `git push` through\n"
            "  bash, and never touch branches other than your own task branch.\n"
            "- Keep the working tree clean of build artifacts and scratch files before\n"
            "  committing (`ls` and remove or .gitignore them; ask yourself whether each\n"
            "  file belongs in review).\n"
            "- Prefer one coherent commit per delivered unit of work; commit again rather\n"
            "  than amending history.\n"
            "- Your final message must name the branch and summarize what a reviewer will\n"
            "  see on it."
        ),
    },
    {
        "name": "conventional-commits",
        "description": "Commit messages follow the Conventional Commits format",
        "match": ["commit", "changelog", "release"],
        "body": (
            "When committing with git_commit_push, write the message as\n"
            "`<type>(<scope>): <imperative summary>`.\n\n"
            "- Types: feat, fix, docs, test, refactor, perf, chore, ci.\n"
            "- The scope is the touched module or area (e.g. `queue`, `web`, `worker`).\n"
            "- Summary in the imperative mood, lower-case, no trailing period, <= 72 chars.\n"
            "- If the change is breaking, append `!` after the type/scope and explain the\n"
            "  break in a body paragraph separated by a blank line.\n\n"
            "Example: `feat(queue): add lease fencing to reaper requeues`"
        ),
    },
    {
        "name": "test-first",
        "description": "Prove changes with a test written before the fix",
        "match": ["test", "bug", "fix", "regression"],
        "body": (
            "Work test-first:\n\n"
            "1. Before changing behavior, write (or extend) a test that fails for the\n"
            "   right reason. Run it with bash and confirm the failure output.\n"
            "2. Make the smallest change that turns it green; run the test again and show\n"
            "   the passing output.\n"
            "3. Run the project's wider test command (make test, pytest, npm test —\n"
            "   whatever the repo uses) before delivering, and include the summary line in\n"
            "   your final message.\n"
            "4. Never delete or weaken an existing assertion to make a test pass; if one\n"
            "   seems wrong, say so in your final message instead."
        ),
    },
]


def upgrade() -> None:
    op.create_table(
        "skills",
        sa.Column("id", sa.dialects.postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column("workspace_id", sa.dialects.postgresql.UUID(as_uuid=True), nullable=False),
        sa.Column("project_id", sa.dialects.postgresql.UUID(as_uuid=True), nullable=False),
        sa.Column("name", sa.String(length=100), nullable=False),
        sa.Column("description", sa.Text(), nullable=False, server_default=""),
        sa.Column("match", sa.dialects.postgresql.JSONB(), nullable=False, server_default="[]"),
        sa.Column("body", sa.Text(), nullable=False, server_default=""),
        sa.Column(
            "created_at", sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False
        ),
        sa.Column(
            "updated_at", sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False
        ),
        sa.ForeignKeyConstraint(["project_id"], ["projects.id"], ondelete="RESTRICT"),
        sa.UniqueConstraint("project_id", "name", name="uq_skills_project_name"),
    )
    op.create_index("ix_skills_project", "skills", ["project_id"])

    insert = sa.text(
        "INSERT INTO skills (id, workspace_id, project_id, name, description, match, body) "
        "VALUES (gen_random_uuid(), CAST(:wid AS uuid), CAST(:pid AS uuid), :name, :desc, "
        "CAST(:match AS jsonb), :body)"
    )
    for seed in _SEEDS:
        op.execute(
            insert.bindparams(
                wid=_WORKSPACE,
                pid=_PROJECT,
                name=seed["name"],
                desc=seed["description"],
                match=json.dumps(seed["match"]),
                body=seed["body"],
            )
        )


def downgrade() -> None:
    op.drop_index("ix_skills_project", table_name="skills")
    op.drop_table("skills")
