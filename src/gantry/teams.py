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
      "autonomous_leader": false,       # true -> editable prompt is core + unlocks spawn
      "gated_tools": ["git_commit_push"],
      "skills": ["test-first"],
      "children": [ <node>, ... ]
    }
"""

from __future__ import annotations

from typing import Any

from gantry.core.models import AgentProfile, Provider, TaskKind
from gantry.prompts import DEFAULT_AUTONOMOUS_LEADER_PROMPT, PLANNER_SYSTEM_PROMPT
from gantry.providers import resolve_model

TeamNode = dict[str, Any]


def _snapshot_model_options(
    profile: AgentProfile, provider: Provider | None
) -> list[dict[str, Any]]:
    """Resolve each menu slug to its full LiteLLM string via the leader's
    provider, so the model the leader names in a spawn is one a worker can run
    directly. Descriptions ride along; keys never do."""
    options: list[dict[str, Any]] = []
    for opt in profile.model_options or []:
        raw = str(opt.get("model") or "").strip()
        if not raw:
            continue
        options.append(
            {
                "model": resolve_model(provider, raw, "") or raw,
                "description": str(opt.get("description") or "").strip(),
            }
        )
    return options


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
        "autonomous_leader": profile.autonomous_leader,
        "model_options": _snapshot_model_options(profile, provider),
        "gated_tools": list(profile.gated_tools),
        "skills": list(profile.skills),
        "children": children,
    }


def node_kind(node: TeamNode) -> TaskKind:
    """Planner iff it orchestrates: an Autonomous Leader always, or an agent that
    may spawn and actually has someone to delegate to. Planner kind carries the
    generous park/wake attempt budget an orchestrator needs."""
    if node.get("autonomous_leader"):
        return TaskKind.PLAN
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
        "prompt, model, and permissions:\n" + lines + "\n\n"
        "After you spawn children you MUST call wait_for_children to sleep until "
        "every one finishes, then integrate their results before you reply. Never "
        "report success while a child is still running — its work would be lost. "
        "Your final message is a handoff that reflects the finished work; if "
        "committing/pushing the result is your responsibility, do it before you "
        "finish (your children may be configured not to)."
    )


def model_menu_text(options: list[dict[str, Any]]) -> str:
    """Appendix listing the models a leader may assign to workers, with the
    operator's when-to-use guidance. The leader passes one of these slugs as
    spawn_subtask's `model` argument; they run on its own provider key."""
    lines = "\n".join(
        f"- {opt['model']}" + (f" — {opt['description']}" if opt.get("description") else "")
        for opt in options
    )
    return (
        "\n\n## Models you can assign\n\n"
        "Pass one of these exact slugs as the `model` argument of spawn_subtask to "
        "run that worker on it (they all use your provider key). Follow the guidance "
        "on when to use each — assign the cheapest model that fits the micro-task:\n" + lines
    )


def node_payload_fields(node: TeamNode) -> dict[str, Any]:
    """The payload keys a task derives from its snapshot node.

    Callers add ``goal`` / ``repo_url`` / ``base_branch`` themselves — those
    are launch-time facts, not profile facts.
    """
    fields: dict[str, Any] = {}
    # The tree node this task embodies — lets the UI light up the running box.
    if node.get("name"):
        fields["agent_name"] = node["name"]
    prompt = node.get("system_prompt")
    if node.get("autonomous_leader"):
        # The leader's behavioral prompt is the operator's own system prompt (edited
        # in the dashboard), falling back to the built-in default only when left
        # blank — there is ONE leader prompt and it is dashboard-managed. The live
        # model menu and team roster are appended dynamically (they are generated
        # from the profile's model_options/children, not hand-typed). The flag also
        # unlocks delegation even with no fixed children (dynamic swarm).
        prompt = prompt or DEFAULT_AUTONOMOUS_LEADER_PROMPT
        if node.get("model_options"):
            prompt += model_menu_text(node["model_options"])
        if node.get("children"):
            prompt += roster_text(node)
    elif node_kind(node) is TaskKind.PLAN:
        prompt = (prompt or PLANNER_SYSTEM_PROMPT) + roster_text(node)
    if prompt:
        fields["system_prompt"] = prompt
    for key in ("model", "provider_id", "max_steps"):
        if node.get(key):
            fields[key] = node[key]
    if node.get("can_spawn") or node.get("autonomous_leader"):
        # Carried so the worker grants delegation tools — to a hands-on agent
        # that also spawns (a coder that delegates to a reviewer), and always to
        # an Autonomous Leader even when its kind would otherwise be EXECUTE.
        fields["can_spawn"] = True
    if node.get("autonomous_leader"):
        # Marks the run for the restricted, read-only + delegation toolset: a
        # pure leader has no write/edit/bash/commit tools, so it must delegate.
        fields["autonomous_leader"] = True
        if node.get("model_options"):
            # Durable copy of the menu (already in the prompt) for inspection.
            fields["model_options"] = node["model_options"]
    if node.get("gated_tools"):
        fields["gated_tools"] = list(node["gated_tools"])
    if node.get("skills"):
        fields["skills"] = list(node["skills"])
    if node.get("children"):
        # The child's own subtree rides along so it can delegate in turn.
        fields["team"] = node
    return fields
