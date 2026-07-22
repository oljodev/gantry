"""Co-pilot tools: an AI proposes a skill or an agent tree for the user to stage.

A co-pilot task runs a specialized agent with a restricted toolset. Its job is
to call ``propose_skill`` / ``propose_tree`` exactly once with a finished
artifact; that emits a ``copilot_proposal`` event the UI stages (Approve injects
it into the editor, Revert undoes it). The agent may use ``ask_user`` to clarify
before proposing.
"""

from __future__ import annotations

from typing import Any, ClassVar

from gantry.core.models import EventType
from gantry.runtime.tools import Tool, ToolContext, ToolIdempotency, ToolResult

SKILL_ARCHITECT_PROMPT = (
    "You are an elite skill architect. A skill is markdown instructions appended to "
    "an agent's system prompt, auto-selected when a run's goal mentions a match "
    "keyword. From the user's high-level description, design ONE robust, production-"
    "grade skill: a clear name (kebab-case), a one-line description, a short list of "
    "match keywords, and a focused, actionable body. Ask a clarifying question with "
    "ask_user only if the request is genuinely ambiguous. When ready, call "
    "propose_skill EXACTLY ONCE with the finished skill, then give a one-sentence "
    "summary. Do not propose more than one skill."
)

TREE_ARCHITECT_PROMPT = (
    "You are an expert multi-agent operations architect. Design a complete, upside-"
    "down agent team tree for the user's goal. Each node is an agent with: name "
    "(kebab-case), role (short), system_prompt (deep and specific — responsibilities, "
    "constraints, and when to delegate), can_spawn (true if it delegates to children), "
    "gated_tools (tools needing human approval, e.g. git_commit_push), skills, and "
    "children. A parent that has children MUST have can_spawn=true. Keep it as small "
    "as the goal requires. Ask a clarifying question with ask_user only if genuinely "
    "needed. When ready, call propose_tree EXACTLY ONCE with the whole tree, then give "
    "a one-sentence summary."
)


class ProposeSkillTool(Tool):
    name = "propose_skill"
    description = (
        "Propose one finished skill for the user to review and apply. Call this exactly "
        "once, with the complete skill."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "name": {"type": "string", "description": "kebab-case skill name"},
            "description": {"type": "string"},
            "match": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Keywords that auto-select this skill by goal.",
            },
            "body": {"type": "string", "description": "Markdown appended to the system prompt."},
        },
        "required": ["name", "body"],
    }
    idempotency = ToolIdempotency.IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        skill = {
            "name": str(arguments.get("name") or "").strip(),
            "description": str(arguments.get("description") or "").strip(),
            "match": [str(m) for m in (arguments.get("match") or [])],
            "body": str(arguments.get("body") or ""),
        }
        if not skill["name"] or not skill["body"]:
            return ToolResult("propose_skill needs a name and a body", is_error=True)
        if ctx.emit_event is not None:
            await ctx.emit_event(EventType.COPILOT_PROPOSAL, {"kind": "skill", "skill": skill})
        return ToolResult("Skill proposed — tell the user it is ready to review and apply.")


class ProposeTreeTool(Tool):
    name = "propose_tree"
    description = (
        "Propose one complete agent team tree for the user to review and apply. Call "
        "this exactly once. Each node: {name, role, system_prompt, can_spawn, "
        "gated_tools[], skills[], children[]}. A node with children must have "
        "can_spawn=true."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "name": {"type": "string", "description": "Team name"},
            "description": {"type": "string"},
            "root": {
                "type": "object",
                "description": (
                    "The root agent node, with nested children[]. Each node has "
                    "name, role, system_prompt, can_spawn (bool), gated_tools[], "
                    "skills[], children[]."
                ),
            },
        },
        "required": ["name", "root"],
    }
    idempotency = ToolIdempotency.IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        root = arguments.get("root")
        if not isinstance(root, dict) or not root.get("name"):
            return ToolResult("propose_tree needs a root node with a name", is_error=True)
        team = {
            "name": str(arguments.get("name") or "").strip(),
            "description": str(arguments.get("description") or "").strip(),
            "root": root,
        }
        if not team["name"]:
            return ToolResult("propose_tree needs a team name", is_error=True)
        if ctx.emit_event is not None:
            await ctx.emit_event(EventType.COPILOT_PROPOSAL, {"kind": "tree", "team": team})
        return ToolResult("Team tree proposed — tell the user it is ready to review and apply.")
