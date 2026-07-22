"""Load project-scoped skills from the database into a SkillRegistry.

Kept separate from :mod:`gantry.skills` so the runtime skill types stay free
of a SQLAlchemy dependency (the loop only ever sees a plain SkillRegistry).
"""

from __future__ import annotations

import uuid

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession

from gantry.core.models import Skill as SkillRow
from gantry.skills import Skill, SkillRegistry


async def load_registry(
    session: AsyncSession, *, workspace_id: uuid.UUID, project_id: uuid.UUID
) -> SkillRegistry:
    """Every skill defined in ``project_id``, as a ready-to-select registry."""
    rows = (
        await session.scalars(
            sa.select(SkillRow).where(
                SkillRow.workspace_id == workspace_id,
                SkillRow.project_id == project_id,
            )
        )
    ).all()
    skills = {
        row.name: Skill(
            name=row.name,
            description=row.description,
            content=row.body,
            match=tuple(row.match),
        )
        for row in rows
    }
    return SkillRegistry(skills)
