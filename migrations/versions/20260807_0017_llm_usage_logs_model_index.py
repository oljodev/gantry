"""index llm_usage_logs for per-model routing lookups

Revision ID: 0017
Revises: 0016
Create Date: 2026-08-07
"""

from __future__ import annotations

from alembic import op

revision: str = "0017"
down_revision: str | None = "0016"
branch_labels: str | None = None
depends_on: str | None = None


def upgrade() -> None:
    # The dynamic-cost router's rolling cache-hit-rate query (see
    # gantry.billing.routing_stats) filters by model_slug and orders by
    # created_at for every routing decision — without this index it is a full
    # table scan of the whole billing audit trail per request.
    op.create_index(
        "ix_llm_usage_logs_model_created",
        "llm_usage_logs",
        ["model_slug", "created_at"],
    )


def downgrade() -> None:
    op.drop_index("ix_llm_usage_logs_model_created", table_name="llm_usage_logs")
