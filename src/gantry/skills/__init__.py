"""Portable skills: SKILL.md files injected into agent system prompts.

A skill is one markdown file with YAML frontmatter::

    ---
    name: conventional-commits
    description: Commit-message conventions for this factory
    match: [commit, changelog]
    ---
    <instructions appended to the system prompt>

Selection is deterministic per task: the payload's explicit ``skills`` list,
plus (unless ``auto_skills`` is false) every skill whose ``match`` keywords
appear in the goal. Injection itself is an event (``skill_injected``) carrying
the full content — the log pins exactly what instructions a run used, even if
the file on disk changes later, and resume rebuilds the identical prompt.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml

from gantry.logging import get_logger

logger = get_logger(__name__)


class SkillParseError(ValueError):
    """The SKILL.md file is malformed (missing frontmatter or name)."""


@dataclass(frozen=True)
class Skill:
    name: str
    description: str
    content: str
    #: Case-insensitive keywords; any hit in the goal auto-selects the skill.
    match: tuple[str, ...] = ()

    def matches(self, goal: str) -> bool:
        haystack = goal.lower()
        return any(keyword.lower() in haystack for keyword in self.match)


def parse_skill_text(text: str, *, source: str = "<memory>") -> Skill:
    if not text.startswith("---"):
        raise SkillParseError(f"{source}: missing YAML frontmatter")
    try:
        _, frontmatter, body = text.split("---", 2)
    except ValueError as exc:
        raise SkillParseError(f"{source}: unterminated frontmatter") from exc
    meta: dict[str, Any] = yaml.safe_load(frontmatter) or {}
    name = str(meta.get("name") or "").strip()
    if not name:
        raise SkillParseError(f"{source}: frontmatter must define a name")
    raw_match = meta.get("match") or []
    if isinstance(raw_match, str):
        raw_match = [raw_match]
    return Skill(
        name=name,
        description=str(meta.get("description") or "").strip(),
        content=body.strip(),
        match=tuple(str(k) for k in raw_match),
    )


def parse_skill_file(path: Path) -> Skill:
    return parse_skill_text(path.read_text(encoding="utf-8"), source=str(path))


class SkillRegistry:
    def __init__(self, skills: dict[str, Skill] | None = None) -> None:
        self._skills = dict(skills or {})

    @classmethod
    def load_dir(cls, root: Path) -> SkillRegistry:
        """Load every ``*.md`` under ``root``; malformed files are skipped loudly."""
        skills: dict[str, Skill] = {}
        if root.is_dir():
            for path in sorted(root.rglob("*.md")):
                if path.name.lower() == "readme.md":
                    continue
                try:
                    skill = parse_skill_file(path)
                except SkillParseError as exc:
                    logger.warning("skills.invalid_file", path=str(path), error=str(exc))
                    continue
                if skill.name in skills:
                    logger.warning("skills.duplicate_name", name=skill.name, path=str(path))
                    continue
                skills[skill.name] = skill
        return cls(skills)

    def get(self, name: str) -> Skill | None:
        return self._skills.get(name)

    def all(self) -> list[Skill]:
        return sorted(self._skills.values(), key=lambda s: s.name)

    def select(self, payload: dict[str, Any]) -> list[Skill]:
        """Deterministic skill set for a task: explicit names, then goal matches."""
        selected: dict[str, Skill] = {}
        for name in payload.get("skills") or []:
            skill = self.get(str(name))
            if skill is None:
                logger.warning("skills.unknown_name", name=str(name))
                continue
            selected[skill.name] = skill
        if payload.get("auto_skills", True):
            goal = str(payload.get("goal") or "")
            for skill in self.all():
                if skill.name not in selected and skill.matches(goal):
                    selected[skill.name] = skill
        return list(selected.values())

    def __len__(self) -> int:
        return len(self._skills)
