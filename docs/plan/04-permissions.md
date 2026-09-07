# 04 — Permission and autonomy modes

## 1. Principles

- **Per chat.** Mode, guard, attachments and grants belong to a chat. Nothing carries over to another chat unless the user creates a project default on purpose.
- **Scope is not permission.** Workspace roots are a boundary enforced by the connectors regardless of mode. Modes and grants decide what happens *inside* the boundary.
- **Every decision is recorded** with its source: mode policy, grant, user (once / for this chat), judge, guardrail, scope, plan mode.
- **Fail closed.** When the machinery that would decide is unavailable (judge timeout, malformed output), ask the user rather than guess.

## 2. Risk tiers

| Tier | Definition | Examples |
|------|------------|----------|
| `read` | Observes; no side effects | `read_file`, `grep`, `view`, Drive `search_files`, read-only shell commands |
| `write` | Mutates local state inside the chat's roots in a way Gantry can revert | `str_replace`, `create`, `write_file`, `move_path` |
| `write_external` | Mutates state outside the machine or outside the roots; not revertible by Gantry | Drive `create_file`, GitHub `create_issue`, a Supabase insert, sending a message |
| `execute` | Runs code with unknown blast radius | `run_command` (unless classified read-only), tools of stdio MCP servers without annotations |
| `destructive` | Irreversible deletion or force operations | `delete_path`, `delete_repository`, `git push --force`, a `DROP TABLE` |
| `app` | Acts only on Gantry's own state and is either pure output or confirmed by its own card | `gantry__create_artifact`, `gantry__propose_memory`, `gantry__propose_skill`, `gantry__search_connectors`, `gantry__request_access` |

Assignment: native manifests declare a tier per tool; the shell connector classifies each command at call time; MCP tools map from annotations with manifest and per-instance overrides (03 §6); unknown tools default conservatively (`write_external` remote, `execute` local). Runtime tools owned by `gantry-agent` (`gantry__…`) are `app` tier by construction: they never touch the user's files, machine or external services, and the ones that persist anything (memory, skills) do so only after the user confirms a card (12).

## 3. Modes

| Tier | Manual | Auto-edit | Plan | Auto · guard off | Auto · guard judge |
|------|--------|-----------|------|------------------|--------------------|
| read | Ask¹ | Allow | Ask¹ | Allow | Allow |
| write | Ask¹ | Allow | Deny² | Allow | Judge |
| write_external | Ask¹ | Ask¹ | Deny² | Allow | Judge |
| execute | Ask¹ | Ask¹ | Ask¹ if the command classifies as read-only, else Deny | Allow | Judge |
| destructive | Ask¹ | Ask¹ | Deny² | Allow³ | Judge³ |
| app | Allow⁴ | Allow⁴ | Allow⁴ | Allow⁴ | Allow⁴ |

¹ unless a standing grant for this chat matches (see §8). ² the tool is not even offered to the model in Plan mode (see §5). ³ `always_confirm` tools and guardrail patterns still ask (see §6). ⁴ never prompts, always logged; see T13 in 01 §8 for why Manual mode's "no exceptions" does not extend to tools whose only effect is Gantry's own UI or a card the user decides on.

Status after M3: the table's mode column is implemented (`gantry-agent/src/permissions.rs`) and Manual mode prompts with **Allow once** and **Deny** (optionally with a message the model sees as `{ "error": "denied_by_user", "message", "hint" }`). Grants (¹) arrive with M7, scope and guardrails with M6 and M7. The judge column is not built until M8; until then a call that would go to the judge asks the user, per the fail-closed rule of §1. `gantry__clock` is `read` tier rather than `app` so that Manual mode has a call to ask about before any connector exists.

**Manual** asks before every call, reads included, exactly as the brief says. It stays usable because every prompt offers "Allow for this chat" with a scope, and that grant is the user's explicit decision.

**Auto-edit** is defined by tier, not by file edits: everything Gantry can undo locally (`write`) is automatic; everything else that changes state asks. So a Supabase write, a GitHub issue or an MCP filesystem server's write all prompt, while a code-editor edit inside the workspace does not. That is the generalization the brief asked for: "auto-edit" means "auto for reversible, local changes".

**Plan** cannot change anything and cannot run anything unclassified, but can ask to read so the plan is grounded in reality (§5).

**Auto** is one mode with a **Guard** setting (§6).

Mode changes mid-chat append a `SystemNote` ("Permission mode is now Plan: propose changes, do not make them") so the model's behavior matches the UI.

## 4. Plan mode specifics

- The tool set offered to the model is filtered to `read` tools, `view`, and `run_command` (whose classifier will deny anything not read-only). Removing write tools from the list is better than denying them: the model does not waste turns trying.
- Reads still prompt, per the brief, and the first prompt offers **Allow all reads for this chat**, a single grant with `tier_ceiling = read`.
- The system note instructs the model to produce a plan (goals, steps, files touched, risks) and to ask before assuming.
- The plan message gets a **Switch to Auto-edit and execute** action; the mode switch is itself a system note, so the model knows the constraint was lifted.

## 5. Auto mode: how the two behaviors are exposed

**Decision:** one mode named Auto with a **Guard** setting: `judge` (default) or `off`. The mode chip reads "Auto · Guarded" or "Auto · Unguarded". Switching to Unguarded shows a one-time confirmation in that chat. Projects can set a default mode and guard for their chats.

Why not two modes: the four-mode mental model (Manual → Auto-edit → Plan → Auto) is the brief's and matches Claude Code; the guard is a safety property of Auto, not a fifth kind of workflow. Making it a setting also lets the guardrail floor and the judge be configured in one place.

**Unguarded Auto** approves everything except the guardrail floor: a short default list of catastrophic patterns (`rm -rf` of `/`, `~` or a root; `git push --force` to a default branch; `mkfs`, `dd of=/dev/…`; piping a download into a shell; recursive deletes; reads or writes of sensitive-path patterns) plus any tool whose manifest sets `always_confirm`. These prompt even in Unguarded Auto. The list is editable in Settings → Guardrails and can be emptied; the default is safe and the off switch is explicit.

## 6. Guarded Auto: the judge

A small, fast model evaluates each non-read tool call and approves or blocks it without interrupting the user.

### Pipeline

Rules run before the judge and are free:

1. Scope violation → deny (no judge, no prompt).
2. `read` tier → allow.
3. Guardrail hard-deny pattern → deny; `always_confirm` → ask.
4. Loop detection: the same tool with the same arguments has failed three times this turn → deny with reason "repeating a failing action".
5. Everything else → judge.

### Model

The judge uses the cheapest fast model of the provider the chat already uses, so no extra key is needed. Defaults ship in `desktop/assets/models/judge_defaults.toml` and are overridable in Settings and per project:

| Chat provider | Default judge model (as of September 2026) |
|---------------|--------------------------------------------|
| Anthropic | `claude-haiku-4-5` |
| OpenAI | the smallest current GPT-5 tier (`gpt-5-nano` today) |
| Google | Gemini 3.x Flash-Lite |
| xAI | `grok-4-fast` (non-reasoning) |
| OpenRouter / custom | user-selected; defaults to a Haiku or Flash-Lite route |

### Inputs (≤ ~2k tokens, stable prefix first so it caches)

- Fixed policy prompt (the rules below), cached.
- Task frame: project name; first user message (≤ 600 chars); latest user message (≤ 600 chars); the assistant's most recent text before this call (≤ 400 chars, its stated intent).
- Workspace roots and the permission mode.
- The action: connector name and its `risk.notes`; tool name and tier; arguments rendered compactly: for shell, the command and cwd; for edits, the path, diff stats and the first 40 diff lines (computed as a dry run before the write); otherwise JSON truncated to 1500 chars.
- Recent history: the last 10 tool calls in this turn as (tool, short args, outcome, decision) plus the count of denials so far.

### Output

Structured output where the provider supports it (Anthropic `output_config.format`, OpenAI JSON schema, Gemini response schema), otherwise JSON in text parsed strictly:

```json
{ "decision": "allow" | "deny", "confidence": 0.0-1.0, "reason": "≤ 200 chars",
  "flags": ["irreversible", "outside_task", "secret_exposure", "loop", "suspicious_input"] }
```

Policy in the prompt: allow actions consistent with the stated task that are in scope or reversible; deny irreversible actions the user did not clearly ask for; deny anything that reads or sends credentials or secrets; deny actions on paths, services or accounts the task never mentioned unless the assistant's intent explains them; deny repeated failing actions; when the action is `destructive` and confidence is below 0.7, do not allow. A low-confidence `allow` on a `destructive` tool becomes a prompt.

### Behavior

- **Allow** → execute; a small "guard ✓" mark on the activity item, with the reason on hover.
- **Deny** → the call is skipped; the model receives `{ "error": "blocked_by_guard", "reason": …, "hint": "Ask the user or choose a safer approach." }`; the activity item shows "Blocked by guard" with an **Allow anyway** button that re-runs the call under a one-time grant; a toast (and a sidebar badge if the user is elsewhere) is the only notification. The judge never opens a blocking prompt.
- **Judge failure** (timeout after 8 s, network error, unparseable output) → fall back to a blocking permission prompt. One interruption in a rare failure beats a silent allow.
- Budget: target latency ≤ 1.5 s per decision; cost roughly a tenth of a cent per decision on Haiku 4.5 with the policy prompt cached, so a heavy coding turn costs cents.
- Audit: `judge.decision` events; Settings → Guard shows recent decisions, override counts and a "this block was wrong" feedback toggle stored for later prompt tuning.

## 7. Permission prompts

`PermissionCard` in the activity feed, at the point in the turn where the call would run:

- Header: connector icon and name, tool name, tier badge, the connector's `risk.notes` on hover.
- Body: what will happen, rendered by kind: a diff preview for edits, the command and cwd for shell, the target for connector calls, and the assistant's last sentence as "why".
- Actions: **Allow once** · **Allow for this chat ▾** (scope: this tool · this path prefix · this command prefix · all reads) · **Deny ▾** (optionally with a message the model will see) · in Manual mode also **Switch to Auto-edit**.
- Keyboard: `Y` allow once, `A` allow for chat, `N` deny. Several pending calls from one parallel batch stack, with **Allow all** for same-tier batches.

The turn waits on the prompt. If the user leaves the chat, the sidebar shows a badge and the prompt is waiting when they return; an OS notification is optional. Cancelling the turn resolves the prompt as cancelled.

## 8. Grants

```
chat_grants: id, chat_id, instance_id, tool_name (NULL = every tool of the instance),
             tier_ceiling (NULL or a tier, used by "all reads"), arg_scope_json,
             source (user_prompt | access_request | project_default), created_at, revoked_at
```

Matching: same chat and instance; tool matches or is wildcard; tier at or under the ceiling when set; `arg_scope` predicate holds (`path_prefix`, `command_prefix`, `pattern`). Grants live as long as the chat and are listed in the chat's **Permissions** panel (mode, guard, roots, attached connectors, grants with revoke, recent decisions). A "Use as project default" action copies a grant into the project's defaults, which new chats in the project inherit; existing chats are untouched.

## 9. Mid-conversation access requests

This is for connectors that are **installed but not attached** to the chat (the connector suggestion tools in 03 §9 handle connectors that are not installed).

- The system prompt carries an inventory: "Attached: filesystem, code-editor, shell. Installed, not attached: github (repositories, issues, pull requests), supabase (database). Call `gantry__request_access` to use one."
- `gantry__request_access { connector, tools?, reason }` is a runtime tool owned by `gantry-agent`, present whenever at least one installed instance is unattached.
- The call becomes `Interaction::AccessRequest`, rendered as `AccessRequestCard`: connector, the tools it wants, its reason; actions **Attach for this chat** · **Attach and allow these tools** · **Deny**.
- On approval: a `chat_connectors` row (and optionally a grant), a `ToolSetChange` system message, and the tool result `{ attached: true, tools: [...] }`; the model's next call sees the tools. Per-call permission still follows the mode, so in Manual mode attaching GitHub does not silently authorize creating issues.

## 10. The Interaction primitive

Permission prompts, access requests, connector suggestions, MCP elicitation, mid-turn re-authentication, and the skill and memory proposals of 12 are all the same mechanism:

```rust
pub struct Interaction {
    pub id: InteractionId, pub chat_id: ChatId, pub turn_id: TurnId,
    pub kind: InteractionKind,        // Permission | AccessRequest | ConnectorSuggestion | Elicitation | AuthRequired | SkillProposal | MemoryProposal
    pub payload: serde_json::Value,   // kind-specific
    pub status: Pending | Resolved | Cancelled | Expired,
    pub resolution: Option<serde_json::Value>,
    pub created_at: Millis, pub resolved_at: Option<Millis>,
}
```

- The backend keeps a `oneshot` sender per pending interaction and persists the row so it survives navigation and restarts (restarts cancel it).
- It travels to the UI as a `decision.requested` event on the turn's channel and as an `interactions:changed` global event for badges.
- `resolve_interaction(id, resolution)` completes it; the turn continues. No timeout by default; an optional auto-deny after N minutes is a setting.
- Cards render from the run store, so they appear immediately, and from `list_pending_interactions` when a chat view mounts.

## 11. Audit

`decision.requested` and `decision.resolved` events carry `source ∈ { mode, grant, user_once, user_chat_grant, judge, guardrail, scope, plan_mode }`. `tool_calls.decision_source` and `tool_calls.judge_json` make "what ran, who allowed it" a single query. The chat's Permissions panel shows the timeline; it can be exported as JSON.
