"""The Autonomous Leader flag: it unlocks delegation at snapshot time and uses the
operator's editable system prompt as the leader's core (falling back to the built-in
default only when blank), and survives the API round-trip."""

from __future__ import annotations

import uuid

import httpx

from gantry.core.models import AgentProfile, Provider, ProviderType, TaskKind
from gantry.prompts import DEFAULT_AUTONOMOUS_LEADER_PROMPT
from gantry.teams import model_menu_text, node_kind, node_payload_fields, snapshot_node

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


def test_leader_falls_back_to_the_default_prompt_and_unlocks_delegation() -> None:
    fields = node_payload_fields(_node(autonomous_leader=True))  # no system_prompt set
    # Delegation is unlocked even though there are no fixed children...
    assert fields["can_spawn"] is True
    # ...the run is marked so the worker gives it the restricted (no-write) toolset...
    assert fields["autonomous_leader"] is True
    # ...and with a BLANK profile prompt the built-in default is the fallback.
    assert fields["system_prompt"] == DEFAULT_AUTONOMOUS_LEADER_PROMPT
    # A leader is always a planner (generous park/wake attempt budget).
    assert node_kind(_node(autonomous_leader=True)) is TaskKind.PLAN


def test_leader_uses_the_editable_system_prompt_as_its_core() -> None:
    # There is ONE leader prompt and it is dashboard-managed: the operator's own
    # system prompt REPLACES the built-in default (it is no longer forced on top).
    fields = node_payload_fields(
        _node(autonomous_leader=True, system_prompt="You are a parser specialist. Delegate.")
    )
    assert fields["system_prompt"] == "You are a parser specialist. Delegate."
    assert DEFAULT_AUTONOMOUS_LEADER_PROMPT not in fields["system_prompt"]


def test_editable_prompt_still_gets_the_dynamic_menu_and_roster_appended() -> None:
    # The live model menu (from model_options) is still appended to whatever core
    # prompt the operator wrote — it is generated, not hand-typed.
    fields = node_payload_fields(
        _node(
            autonomous_leader=True,
            system_prompt="Custom leader brain.",
            model_options=[{"model": "openrouter/x", "description": "cheap"}],
        )
    )
    assert fields["system_prompt"].startswith("Custom leader brain.")
    assert "## Models you can assign" in fields["system_prompt"]


def test_non_leader_is_unchanged() -> None:
    # A plain agent stays a static EXECUTE node with no forced prompt/delegation.
    fields = node_payload_fields(_node())
    assert node_kind(_node()) is TaskKind.EXECUTE
    assert "can_spawn" not in fields
    assert "system_prompt" not in fields


def test_snapshot_resolves_menu_slugs_and_injects_them_into_the_prompt() -> None:
    provider = Provider(
        workspace_id=uuid.uuid4(),
        name="or",
        provider_type=ProviderType.OPENROUTER,
        default_model="x",
    )
    profile = AgentProfile(
        workspace_id=uuid.uuid4(),
        name="lead",
        role="swarm",
        autonomous_leader=True,
        model_options=[
            {"model": "deepseek/deepseek-chat", "description": "cheap; splits, reads, QA"},
            {"model": "deepseek/deepseek-r1", "description": "expensive; hard algorithms only"},
        ],
        gated_tools=[],
        skills=[],
    )
    node = snapshot_node(profile, provider, [])
    # Each slug is resolved to the provider's LiteLLM string at snapshot time.
    assert node["model_options"][0]["model"] == "openrouter/deepseek/deepseek-chat"

    fields = node_payload_fields(node)
    prompt = fields["system_prompt"]
    # The menu (resolved slugs + guidance) is injected into the leader prompt...
    assert "## Models you can assign" in prompt
    assert "openrouter/deepseek/deepseek-r1" in prompt
    assert "hard algorithms only" in prompt
    # ...and a durable copy rides in the payload for inspection.
    assert fields["model_options"] == node["model_options"]


def test_no_menu_no_injection() -> None:
    # A leader with an empty menu gets no "Models you can assign" section.
    fields = node_payload_fields(_node(autonomous_leader=True))
    assert "Models you can assign" not in fields["system_prompt"]
    assert "model_options" not in fields


def test_model_menu_text_lists_slugs_with_guidance() -> None:
    text = model_menu_text([{"model": "openrouter/a", "description": "for X"}])
    assert "openrouter/a" in text and "for X" in text
    assert "spawn_subtask" in text  # tells the leader how to use them


async def test_flag_round_trips_through_the_api(client: httpx.AsyncClient) -> None:
    agent = await create_agent(client, "swarm-lead", autonomous_leader=True)
    assert agent["autonomous_leader"] is True

    fetched = (await client.get("/api/agents")).json()["agents"][0]
    assert fetched["autonomous_leader"] is True

    # Defaults to false when omitted.
    plain = await create_agent(client, "plain")
    assert plain["autonomous_leader"] is False


async def test_leader_default_prompt_endpoint(client: httpx.AsyncClient) -> None:
    # The dashboard fetches this to load the built-in default into the editable
    # prompt field so the operator can start from it and customize.
    resp = await client.get("/api/agents/leader-default-prompt")
    assert resp.status_code == 200
    assert resp.json()["system_prompt"] == DEFAULT_AUTONOMOUS_LEADER_PROMPT


async def test_model_options_round_trip_through_the_api(client: httpx.AsyncClient) -> None:
    menu = [
        {"model": "deepseek/deepseek-chat", "description": "cheap; mechanical work"},
        {"model": "deepseek/deepseek-r1", "description": "hard reasoning only"},
    ]
    agent = await create_agent(client, "menu-lead", autonomous_leader=True, model_options=menu)
    assert agent["model_options"] == menu

    fetched = (await client.get("/api/agents")).json()["agents"][0]
    assert fetched["model_options"] == menu

    # Defaults to an empty menu when omitted.
    plain = await create_agent(client, "no-menu")
    assert plain["model_options"] == []
