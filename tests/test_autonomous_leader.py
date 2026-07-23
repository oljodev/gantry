"""The Autonomous Leader flag: it forces the swarm-master prompt and unlocks
delegation at snapshot time, and survives the API round-trip."""

from __future__ import annotations

import httpx

from gantry.core.models import TaskKind
from gantry.runtime.state import AUTONOMOUS_LEADER_PROMPT
from gantry.teams import node_kind, node_payload_fields

from .test_agents_teams_api import create_agent


def _node(**overrides: object) -> dict[str, object]:
    node: dict[str, object] = {
        "name": "lead",
        "role": "runs the swarm",
        "system_prompt": None,
        "can_spawn": False,
        "autonomous_leader": False,
        "children": [],
    }
    node.update(overrides)
    return node


def test_leader_forces_prompt_and_unlocks_delegation_without_children() -> None:
    fields = node_payload_fields(_node(autonomous_leader=True))
    # Delegation is unlocked even though there are no fixed children...
    assert fields["can_spawn"] is True
    # ...and the leader prompt is the core instruction.
    assert fields["system_prompt"] == AUTONOMOUS_LEADER_PROMPT
    # A leader is always a planner (generous park/wake attempt budget).
    assert node_kind(_node(autonomous_leader=True)) is TaskKind.PLAN


def test_leader_prompt_overrides_a_custom_prompt_but_keeps_it_as_context() -> None:
    fields = node_payload_fields(
        _node(autonomous_leader=True, system_prompt="Only touch the parser.")
    )
    prompt = fields["system_prompt"]
    assert prompt.startswith(AUTONOMOUS_LEADER_PROMPT)  # leader discipline dominates
    assert "Only touch the parser." in prompt  # custom instruction not lost


def test_non_leader_is_unchanged() -> None:
    # A plain agent stays a static EXECUTE node with no forced prompt/delegation.
    fields = node_payload_fields(_node())
    assert node_kind(_node()) is TaskKind.EXECUTE
    assert "can_spawn" not in fields
    assert "system_prompt" not in fields


async def test_flag_round_trips_through_the_api(client: httpx.AsyncClient) -> None:
    agent = await create_agent(client, "swarm-lead", autonomous_leader=True)
    assert agent["autonomous_leader"] is True

    fetched = (await client.get("/api/agents")).json()["agents"][0]
    assert fetched["autonomous_leader"] is True

    # Defaults to false when omitted.
    plain = await create_agent(client, "plain")
    assert plain["autonomous_leader"] is False
