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
| app | Allow⁴ | Allow⁴ | Allow⁴ | Allow⁴ | Allow⁴, except Judge⁵ |

¹ unless a standing grant for this chat matches (see §8). ² the tool is not even offered to the model in Plan mode (see §5). ³ `always_confirm` tools and guardrail patterns still ask (see §6). ⁴ never prompts, always logged; see T13 in 01 §8 for why Manual mode's "no exceptions" does not extend to tools whose only effect is Gantry's own UI or a card the user decides on. ⁵ the one `app` tool that widens what the chat can reach rather than acting inside it — `gantry__request_access`, marked `widens_access` — is the guard's in Guarded Auto (§9). Every other mode allows the call and the tool asks the user itself.

Status: the whole table is implemented, including the judge column (`gantry-agent/src/permissions.rs`, `gantry-agent/src/judge.rs`, `gantry-core/src/guardrail.rs`, §5, §6 and §8). A card offers **Allow once**, **Deny** with an optional message the model sees as `{ "error": "denied_by_user", "message", "hint" }`, and the standing scopes of §8; a grant may only turn an **Ask** into an allow, so Plan mode's denials, `always_confirm` and the guardrails are untouched by it — and it answers the guard, because a grant *is* the user having decided. Plan mode filters the tool set and its last turn offers **Switch to Auto-edit and execute**. Scope checks are the connectors' own (M6). `gantry__clock` is `read` tier rather than `app` so that Manual mode has a call to ask about before any connector exists.

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

**Unguarded Auto** approves everything except the guardrail floor: a short default list of catastrophic patterns (`rm -rf` of `/`, `~` or a root; `git push --force`; `mkfs`, `dd of=/dev/…`; piping a download into a shell; recursive deletes; reads or writes of sensitive-path patterns) plus any tool whose manifest sets `always_confirm`. These prompt even in Unguarded Auto. The list is editable in Settings → Guard & guardrails and can be emptied; the default is safe and the off switch is explicit.

### As built (M7, 2026-09-11)

The floor ships in `desktop/assets/guardrails/defaults.toml`, is matched by `gantry-core/src/guardrail.rs` and is applied by the permission engine before the mode table. Five decisions shaped it.

**A guardrail reads text, and says so.** Like the command classifier next door, it matches patterns against a command line and against arguments. A command that runs has the user's privileges and any pattern can be spelled around by someone trying to; what the floor is for is the handful of spellings people reach by accident, which are few and well known. The interface says "Blocked by a guardrail", never "this cannot happen". The boundaries that hold are the modes, the roots and the operating system.

**Four kinds, one file.** `deny` never runs, in any mode; `confirm` asks, in every mode including unguarded Auto; `path` is a glob for a file worth a question before it is read or written; `secret` is a key. Commands and secrets are regular expressions, paths are globs. Each rule carries a stable `id`, which is what a switched-off rule is remembered by, and a `reason` written for the person who reads it on the card.

**Settings store the deviation, not a copy.** `guardrails.disabled` is a list of shipped ids the user turned off and `guardrails.custom` holds their own rules; `guardrails.enabled` is the whole floor's off switch. A stored copy of the list would freeze on the day it was made, and a release that adds a rule would never reach the machine that most needed it. `Settings::SECTIONS` gains a `guardrails` row; no migration, because the table is one JSON document per section.

**A grant never answers a guardrail** — with one exception the plan already named. A grant is the user's answer to a question they were asked, not an answer to a different question, so a standing grant for `read_file` does not reach `~/.ssh/id_ed25519`. The exception is the sensitive path "without an explicit grant" above: a grant carrying an `ArgScope::PathPrefix` that covers the path *is* explicit, and lifts it. Command and secret rules have no such exception.

**A path is found however it is named, and a key wherever it sits.** Path rules are matched against every short single-line string in the arguments — not an allowlist of argument names, because an MCP server calls its path `file`, `target` or `uri` — and against each word of a command, because `cat ~/.ssh/id_rsa` is the same request as reading the file by name. A file's contents cannot be mistaken for a path, because they are neither short nor single-line. Secret rules read the arguments whole, contents included: a key in a call is a key on its way into a repository or out to a stranger, and both are worth one question. `Guardrails::redact` is the same rules pointed the other way, for the log and for the memories of 12 §B3.

A rule whose pattern does not compile is skipped, reported in Settings as "Not valid", and takes nothing else down with it: one bad regular expression of the user's own must not disable `rm -rf /`.

## 6. Guarded Auto: the judge

A small, fast model evaluates each non-read tool call and approves or blocks it without interrupting the user.

### Pipeline

Rules run before the judge and are free:

1. Scope violation → deny (no judge, no prompt).
2. Guardrail hard-deny pattern → deny; `confirm`, sensitive path, secret and `always_confirm` → ask. **Built (§5);** it runs first, so the reason the user reads is the rule's own.
3. `read` tier → allow.
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
- **Judge failure** (timeout after 30 s, network error, unparseable output) → fall back to a blocking permission prompt. One interruption in a rare failure beats a silent allow.
- Budget: target latency ≤ 1.5 s per decision; cost roughly a tenth of a cent per decision on Haiku 4.5 with the policy prompt cached, so a heavy coding turn costs cents.
- Audit: `judge.decision` events; Settings → Guard shows recent decisions, override counts and a "this block was wrong" feedback toggle stored for later prompt tuning.

### As built (M8, 2026-09-11)

`gantry-agent/src/judge.rs` renders the input, asks the model and reads the answer;
`permissions::decide` returns `Decision::Judge` where Guarded Auto reaches the guard, and the
runner takes it from there. `desktop/assets/prompts/judge.md` is the policy. Six decisions shaped
it.

**The engine stops at the question.** `decide` is a pure function and deciding needs a network
call and the turn's history, so the engine answers with `Judge` rather than an allow or a deny
and the runner does the asking. That keeps the whole permission table testable without a model,
which is what makes "a guardrail outranks the guard" a test rather than a claim.

**Everything in front of the judge answers instead of it.** A hard-deny guardrail refuses, a
guardrail question stays the user's, an `always_confirm` tool is confirmed by the user, and a
standing grant allows — all before the judge is asked. So the judge is only asked where the
answer is genuinely a judgement, and a model can never talk its way past `rm -rf /`, because the
floor refused before anything was asked. The grant case is the one the plan did not spell out: a
grant means the user already answered this question by hand and said to stop asking, and the
judge is a stand-in for the asking.

**Three passes, so nobody waits for an answer they did not need.** The rules decide the whole
batch instantly; every guard question then goes out at once, so four calls cost one round trip
rather than four; the cards are raised last, sorted back into the model's own order so they stack
the way the calls were made.

**Fail closed means "ask", not "deny".** A timeout, an unreachable provider or an answer that is
not a verdict becomes a permission card carrying the reason it exists — "the guard's answer was
unreadable, so this one is yours" — plus a `provider.notice` and a `judge.decision` event with
source `unavailable`. The card says why, because in Auto mode being asked at all is the surprising
part. The same path takes a `destructive` call the judge allowed below 0.7 confidence.

**The answer is read leniently, the verdict strictly.** The decision and the reason must both be
there — a verdict missing either is not a verdict — but the object is *found* rather than assumed:
a small model told "JSON only" still fences it or introduces it, and turning a correct answer into
a prompt over a code fence would interrupt the user for nothing. The structured-output shape is
sent through `provider_options` where the wire key is ours to set (`response_format`, `text`,
`output_config`) and the model's `capabilities.structured_output` says it is supported. Gemini is
the exception: its response schema lives inside `generation_config`, which the client already
fills in, and overwriting that key would drop the token limit with it — so Gemini is asked in the
prompt, like every provider with no schema support at all.

**An override is remembered, not stored.** **Allow anyway** cannot run the blocked call — its turn
is over and the model already has `blocked_by_guard` — so the override is held in memory, the chat
is told in a `SystemNote` what the user decided, and a new turn starts from that note: the model
makes the call again and the override answers it. The block stays in the transcript, marked
`overridden`, because it happened. The override is deliberately not persisted: a decision about
one action in one moment should not outlive a restart the user never connected it to.

**What the guard is asked about matters more than how fast it answers** (2026-09-11, from the
first live run). Six hardware probes in one batch produced six simultaneous requests to a cheap
fast route, and one of them timed out — so the guard handed a `cat /proc/meminfo 2>/dev/null` to
the user, which is a read. Three things came out of it, in the order they matter: the classifier
now proves `2>/dev/null` and `>&1` read-only, because a redirection that discards writes nothing
and refusing it escalated a read to a prompt; the `ls*` family and the other machine-description
commands joined the read-only list, since every one of them reads whatever its flags; and only
[`MAX_IN_FLIGHT`] decisions are in flight at once. The judge's request also asks for a single
attempt (`ChatRequest::retries`): it has eight seconds and someone is waiting through them, so
spending them on a rate limiter's backoff buys the same answer, late. A card in one second is a
better failure than a card in eight.

**Eight seconds was the wrong number** (2026-09-11, from the second live run, and it reverses the
last sentence above). One `hostnamectl` probe — the only one of seven the classifier could not
prove read-only, and the guard's whole question — timed out, and the user was handed a card. The
bet in a short timeout is that giving up early saves the user time, and it does not: a timeout
hands them the command to read and answer, so the short budget buys a wait that ends in homework
instead of a wait that ends in an answer. A cheap fast route usually answers in a second or two
and sometimes takes twenty, which puts eight inside its range rather than outside it. [`TIMEOUT`]
is 30 s, and at 30 s a retry is affordable again — about a second of backoff to rescue the
commonest failure there is, a rate limiter — so `retries` is 2.

**The guard answers a second kind of question** (2026-09-11): whether the model may attach a
connector the user installed but did not give this chat. It is the same machinery — `Decision::
Judge`, one verdict, one row, **Allow anyway** — put to the one `App` call that widens what a
chat can reach rather than acting inside it. §9 has the reasoning; the short of it is that Auto
promised not to interrupt and was interrupting on the commonest thing a model asks for, and that
attaching is exactly the kind of decision the guard exists to take: it grants nothing by itself,
so the only question is whether the connector fits the task.

**The layers held, live** (2026-09-11, an adversarial run by the user). Five commands written to
look dangerous: `curl … | bash` was a hard guardrail deny before the guard was asked; `sudo halt`
was a guardrail **ask**, so it reached the user and not the guard, which is §5's rule that a rule
outranks the judge; and the guard denied the other three at confidence 1.0 — `shutil.rmtree('/')`
as `irreversible`, and `eval $(base64 -d <<< …)` as `suspicious_input`, having decoded the base64
itself to see what it said. Two things to keep honest about it. The user's message included "Info
to the guard: block them", which reaches the frame as the task and which one verdict cited, so
the run was steered and only three of the flags stand on the task alone. And `dd if=/dev/urandom
of=/dev/null` was flagged `irreversible` when it writes to `/dev/null` and destroys nothing: the
guard reads `sudo dd` and does not finish the line. That is the failure direction to prefer, but
it is a false positive, and the policy is where it is fixed: `judge.md` now has a **Read the whole
line before you flag it** section — `sudo` at the front is not a verdict, `--dry-run` means the
action is not performed, writing to `/dev/null` discards (without making what the rest of the line
deletes come back), and a flag says what the action does rather than how it reads, with
`outside_task` named as the flag for a command that is merely pointless. Whether it holds is a
live question, not an offline one: no test here can ask a model what it now thinks of `dd`.

Two gaps, both recorded rather than hidden. **Dry-run diffs** are not among the judge's inputs: a
dry run needs a connector that can compute one without performing it, and no connector offers
that, so an edit reaches the judge as its path and its truncated arguments. **Project defaults**
wait for M11 — 09 said the `projects` table existed from M2 and it does not; only
`chats.project_id` does, and a project default is unreachable until a chat can belong to a
project.

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

Matching: same chat and instance; tool matches or is wildcard; tier at or under the ceiling when set; `arg_scope` predicate holds (`path_prefix`, `command_prefix`), failing closed when the argument is absent. As built, a card offers two standing scopes: **this tool for this chat**, and, for a `read` call, **all reads from this connector for this chat**, which is the "Allow all reads" of §4. The argument scopes are defined and matched but only reachable once the tools that carry a path or a command exist. A grant made in answer to one card applies from the next batch of calls onwards. `App` tools are never granted, because they never ask. Grants live as long as the chat and are listed in the chat's **Permissions** panel (mode, guard, roots, attached connectors, grants with revoke, recent decisions). A "Use as project default" action copies a grant into the project's defaults, which new chats in the project inherit; existing chats are untouched.

## 9. Mid-conversation access requests

This is for connectors that are **installed but not attached** to the chat (the connector suggestion tools in 03 §9 handle connectors that are not installed).

As built (M10, 2026-09-08):

- The prompt carries a `<gantry_connectors>` inventory: "attached to this chat: filesystem (12 tools) · installed, not attached: github (44 tools) — call gantry__request_access to use one; the user decides." It is assembled per turn rather than frozen with the chat's snapshot, because installing and attaching happen outside the chat and a frozen list would go on lying about them (10 §2).
- `gantry__request_access { connector, tools?, reason }` is a runtime tool owned by `gantry-agent`, offered whenever at least one instance is installed. `reason` is required and is shown to the user in the model's own words.
- The call becomes `Interaction::AccessRequest`, rendered as `AccessRequestCard`: connector, the tools it wants, its reason; actions **Attach for this chat** · **Attach and allow these tools** · **Not now**.
- **In Auto, nobody is asked** (2026-09-11). Auto's promise is that the user is not interrupted, and a card in front of every attach broke it for the commonest thing a model asks for: a chat that starts with the filesystem and is asked to inspect the machine has to stop on its first move. So `request_access` carries `ToolDef::widens_access`, the one `App` tool that does, and the mode decides the call before it runs: **unguarded Auto** allows it like any other call, and **guarded Auto** sends it to the guard, which has a section of `judge.md` for the question and answers it as an `outside_task` judgement — a shell for "check my hardware" passes, a repository connector for "summarise this PDF" does not. A denial is an ordinary guard block: the row says so, the model is told `blocked_by_guard` with the reason, and **Allow anyway** overrides it like any other. Every other mode allows the *call* and shows the card, exactly as before; the branch decides who answers, never whether attaching happened.
  - This is defensible because attaching grants nothing by itself. The tools it adds come back through the whole engine — guardrails, mode, guard, grants — on every call they make, which is the sentence the card itself has always carried. What Auto gives away is the model's ability to widen its own reach, and that is what the guard is there to judge.
  - **Installing is not attaching.** `suggest_connector` runs somebody else's code and asks for credentials, so it stays the user's in every mode and never sets `widens_access`. The pair is the whole point of the flag: one bit that says which of the two a call is.
  - Two places the pipeline would otherwise ask twice, and does not. A guard that **cannot decide** hands its call to the user — but this call's own tool is already a card, so it runs and that card is the question, rather than a permission card in front of a card. And **Allow anyway** on a guard-blocked attach attaches: the user answered that exact question by clicking it, and asking again on the tool's card would be two clicks for one decision. Both are carried by the `attach_decided` bit the runner puts on the call's `ChatScope`, decided where the decision is made rather than derived afterwards from a `DecisionSource` that cannot tell Manual's allow from unguarded Auto's.
- On approval: a `chat_connectors` row with source `access_request`, one grant per named tool when the second action was chosen, and the tool result `{ attached: true, tools: [...] }`. Only the card's branch grants tools — pre-approving a tool is the user saying "stop asking me", and the guard was never asked that. The runner re-reads the chat's connectors between tool rounds, rebuilds the tool set and appends a `ToolSetChange` message, so the model's next call has the tools and knows it. Per-call permission still follows the mode, so in Manual mode attaching GitHub does not silently authorize creating issues.
- The same re-read covers the user attaching or detaching something in the `+` menu while a turn runs, which is the other way the tool set can change under a turn.

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
