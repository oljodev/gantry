"""Skills: SKILL.md parsing, registry matching, and durable injection."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import httpx
import pytest
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import DEFAULT_WORKSPACE_ID, EventType, Task, TaskKind
from gantry.runtime.loop import run_agent_task
from gantry.runtime.state import rehydrate
from gantry.runtime.tools import ToolRegistry
from gantry.skills import Skill, SkillParseError, SkillRegistry, parse_skill_text

from .fakes import ScriptedLLM, final_response

Sessions = async_sessionmaker[AsyncSession]

REPO_ROOT = Path(__file__).resolve().parent.parent

SAMPLE = """---
name: sample-skill
description: A sample
match: [widget, gadget]
---
Always use widgets responsibly.
"""


def test_parse_skill_text() -> None:
    skill = parse_skill_text(SAMPLE)
    assert skill.name == "sample-skill"
    assert skill.description == "A sample"
    assert skill.match == ("widget", "gadget")
    assert skill.content == "Always use widgets responsibly."


@pytest.mark.parametrize(
    "text",
    [
        "no frontmatter at all",
        "---\ndescription: nameless\n---\nbody",
        "---\nname: x",  # unterminated
    ],
)
def test_malformed_skill_files_are_rejected(text: str) -> None:
    with pytest.raises(SkillParseError):
        parse_skill_text(text)


def test_registry_loads_dir_and_skips_bad_files(tmp_path: Path) -> None:
    (tmp_path / "good.md").write_text(SAMPLE)
    (tmp_path / "bad.md").write_text("not a skill")
    (tmp_path / "dupe.md").write_text(SAMPLE)  # duplicate name → skipped
    registry = SkillRegistry.load_dir(tmp_path)
    assert len(registry) == 1
    assert registry.get("sample-skill") is not None


def test_builtin_skills_load() -> None:
    registry = SkillRegistry.load_dir(REPO_ROOT / "skills")
    names = {s.name for s in registry.all()}
    assert {"conventional-commits", "test-first", "branch-hygiene"} <= names


def test_selection_is_explicit_plus_goal_matching() -> None:
    a = Skill(name="a", description="", content="A!", match=("widget",))
    b = Skill(name="b", description="", content="B!", match=("nothing-here",))
    registry = SkillRegistry({"a": a, "b": b})

    assert [s.name for s in registry.select({"goal": "polish the widget"})] == ["a"]
    assert [s.name for s in registry.select({"goal": "x", "skills": ["b"]})] == ["b"]
    # Explicit first, then matches; unknown names are skipped; opt-out works.
    both = registry.select({"goal": "widget work", "skills": ["b", "ghost"]})
    assert [s.name for s in both] == ["b", "a"]
    assert registry.select({"goal": "widget work", "auto_skills": False}) == []


async def enqueue(db: Sessions, payload: dict[str, Any]) -> Task:
    async with session_scope(db) as session:
        return await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"model": "fake/test", **payload},
        )


async def test_injection_is_durable_and_visible_to_the_llm(db: Sessions) -> None:
    widget_skill = Skill(name="widgets", description="d", content="WIDGET RULES", match=("widget",))
    registry = SkillRegistry({"widgets": widget_skill})
    task = await enqueue(db, {"goal": "improve the widget"})
    llm = ScriptedLLM([final_response("done")])
    await run_agent_task(db, task, llm, ToolRegistry([]), skills=registry)

    # The LLM saw the injected instructions in its system message.
    system = llm.calls[0]["messages"][0]
    assert "WIDGET RULES" in system["content"] and "## Skill: widgets" in system["content"]

    async with session_scope(db) as session:
        events = await read_events(session, task.id)
    injected = [e for e in events if e.event_type is EventType.SKILL_INJECTED]
    assert len(injected) == 1
    assert injected[0].payload["content"] == "WIDGET RULES"  # pinned verbatim

    # Rehydration rebuilds the byte-identical system prompt.
    state = rehydrate(task.payload, events)
    assert state.messages[0]["content"] == system["content"]


async def test_reinjection_never_duplicates(db: Sessions) -> None:
    registry = SkillRegistry(
        {"widgets": Skill(name="widgets", description="d", content="W", match=("widget",))}
    )
    task = await enqueue(db, {"goal": "widget", "max_steps": 5})
    await run_agent_task(
        db, task, ScriptedLLM([final_response("one")]), ToolRegistry([]), skills=registry
    )
    # A retry/resume of the same task selects the same skill but must not
    # re-inject it (the event log already carries it).
    llm2 = ScriptedLLM([final_response("two")])
    await run_agent_task(db, task, llm2, ToolRegistry([]), skills=registry)
    async with session_scope(db) as session:
        events = await read_events(session, task.id)
    assert sum(1 for e in events if e.event_type is EventType.SKILL_INJECTED) == 1
    assert llm2.calls[0]["messages"][0]["content"].count("## Skill: widgets") == 1


async def test_skills_api_create_and_list(client: httpx.AsyncClient) -> None:
    created = await client.post(
        "/api/skills",
        json={"name": "widgets", "description": "d", "match": ["widget"], "body": "RULES"},
    )
    assert created.status_code == 201, created.text
    assert created.json()["body"] == "RULES"

    listed = (await client.get("/api/skills")).json()["skills"]
    assert [s["name"] for s in listed] == ["widgets"]
