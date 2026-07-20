"""Team snapshots: from profile rows to self-contained task payloads.

A team launch **recursively snapshots** the whole profile tree into the root
task's payload (``payload["team"]``). From that moment the run depends only
on the immutable payload: editing or deleting profiles can never leak into an
in-flight tree, and ``spawn_subtask`` re-runs during crash recovery rebuild
byte-identical child payloads with zero DB reads.

Snapshot node shape (recursive)::

    {
      "name": "coder",                 # profile name — the spawn key
      "role": "Senior implementer",
      "system_prompt": "..." | null,   # null -> kind default at runtime
      "provider_id": "uuid" | null,    # worker resolves key/base_url via vault
      "model": "openrouter/..." | null,  # ALREADY LiteLLM-mapped
      "max_steps": 40 | null,
      "can_spawn": true,
      "gated_tools": ["git_commit_push"],
      "skills": ["test-first"],
      "children": [ <node>, ... ]
    }
"""

from __future__ import annotations

from typing import Any

from gantry.core.models import AgentProfile, Provider, TaskKind
from gantry.providers import resolve_model
from gantry.runtime.state import PLANNER_SYSTEM_PROMPT

TeamNode = dict[str, Any]


def snapshot_node(
    profile: AgentProfile,
    provider: Provider | None,
    children: list[TeamNode],
) -> TeamNode:
    """One resolved tree node; ``model`` is finalized to a LiteLLM string here."""
    model = resolve_model(provider, profile.model, "") or None
    return {
        "name": profile.name,
        "role": profile.role,
        "system_prompt": profile.system_prompt,
        "provider_id": str(profile.provider_id) if profile.provider_id else None,
        "model": model,
        "max_steps": profile.max_steps,
        "can_spawn": profile.can_spawn,
        "gated_tools": list(profile.gated_tools),
        "skills": list(profile.skills),
        "children": children,
    }


def node_kind(node: TeamNode) -> TaskKind:
    """Planner iff it may spawn and actually has someone to delegate to."""
    if node.get("can_spawn") and node.get("children"):
        return TaskKind.PLAN
    return TaskKind.EXECUTE


def roster_text(node: TeamNode) -> str:
    """Appendix telling a planner who its delegates are and how to spawn them."""
    lines = "\n".join(
        f"- {child['name']}: {child.get('role') or 'no description'}"
        for child in node.get("children", [])
    )
    return (
        "\n\n## Your team\n\n"
        "Delegate by calling spawn_subtask with the `agent` argument set to one "
        "of these names — the child then runs with that agent's own system "
        "prompt, model, and permissions:\n" + lines
    )


def node_payload_fields(node: TeamNode) -> dict[str, Any]:
    """The payload keys a task derives from its snapshot node.

    Callers add ``goal`` / ``repo_url`` / ``base_branch`` themselves — those
    are launch-time facts, not profile facts.
    """
    fields: dict[str, Any] = {}
    prompt = node.get("system_prompt")
    if node_kind(node) is TaskKind.PLAN:
        prompt = (prompt or PLANNER_SYSTEM_PROMPT) + roster_text(node)
    if prompt:
        fields["system_prompt"] = prompt
    for key in ("model", "provider_id", "max_steps"):
        if node.get(key):
            fields[key] = node[key]
    if node.get("gated_tools"):
        fields["gated_tools"] = list(node["gated_tools"])
    if node.get("skills"):
        fields["skills"] = list(node["skills"])
    if node.get("children"):
        # The child's own subtree rides along so it can delegate in turn.
        fields["team"] = node
    return fields
