"""multi-modal prompt attachments

Revision ID: 0014
Revises: 0013
Create Date: 2026-08-06
"""

from __future__ import annotations

import sqlalchemy as sa
from alembic import op
from sqlalchemy.dialects.postgresql import UUID

revision: str = "0014"
down_revision: str | None = "0013"
branch_labels: str | None = None
depends_on: str | None = None


def upgrade() -> None:
    op.create_table(
        "attachments",
        sa.Column("id", UUID(as_uuid=True), primary_key=True),
        sa.Column("workspace_id", UUID(as_uuid=True), nullable=False),
        sa.Column(
            "project_id",
            UUID(as_uuid=True),
            sa.ForeignKey("projects.id", ondelete="CASCADE"),
            nullable=False,
        ),
        sa.Column("filename", sa.String(255), nullable=False),
        sa.Column("media_type", sa.String(128), nullable=False),
        sa.Column("kind", sa.String(16), nullable=False),
        sa.Column("size_bytes", sa.BigInteger(), nullable=False, server_default=sa.text("0")),
        sa.Column("digest", sa.String(64), nullable=False),
        sa.Column("storage_key", sa.Text(), nullable=False),
        sa.Column("extracted_text", sa.Text(), nullable=False, server_default=""),
        sa.Column("extract_error", sa.Text(), nullable=False, server_default=""),
        sa.Column("pages", sa.Integer(), nullable=False, server_default=sa.text("0")),
        sa.Column("transcript", sa.Text(), nullable=False, server_default=""),
        sa.Column("transcript_model", sa.String(200), nullable=False, server_default=""),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            nullable=False,
            server_default=sa.func.now(),
        ),
    )
    op.create_index("ix_attachments_workspace", "attachments", ["workspace_id"])
    op.create_index("ix_attachments_project", "attachments", ["project_id"])
    op.create_index("ix_attachments_digest", "attachments", ["workspace_id", "digest"])


def downgrade() -> None:
    op.drop_index("ix_attachments_digest", table_name="attachments")
    op.drop_index("ix_attachments_project", table_name="attachments")
    op.drop_index("ix_attachments_workspace", table_name="attachments")
    op.drop_table("attachments")
