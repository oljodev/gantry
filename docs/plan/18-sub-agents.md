# 18 — Sub agents

A model in Gantry can hand part of a job to another model: "read these forty pages and tell me
which three matter", "work through this refactor while I keep talking to the user". The model
doing the handing is the **parent**; the ones it starts are **sub agents**. A sub agent is a
conversation the user never speaks in, with its own system prompt, its own tools and its own
transcript, whose whole output is one report handed back to the parent as a tool result.

This document settles what a sub agent is, who decides its shape, what happens when one wants
permission, where the user watches it, and what lands in which phase. It was designed with Olav
on 2026-09-17, answering sixteen questions; his answers are the decisions below.

Sub agents are **item 13** of the build order and the last of Olav's own feature additions.

---

## 1. Why this is not a connector, and not the code surface either

A connector is an integration with a system outside Gantry. Sub agents are Gantry talking to
itself: no server, no account, no key, nothing to install and nothing that can be missing. They
are closer to artifacts (13) or to the code surface's tools (16 §8) — app-owned capability.

But the *mechanism* of a connector is exactly right. A chat decides whether sub agents are
available the same way it decides whether GitHub is: a checkbox in the composer's + menu, a tool
namespace in the turn's tool list, a risk tier on every call. Rebuilding that as a special case
would mean a second path through the turn loop for one feature.

So: **implemented as a native connector, presented as part of the app.** It is installed on first
run and cannot be removed (03 §11, `catalog.install_by_default`), it is hidden from Discover and
from Your connectors (`catalog.hidden`, new here), it has no card and no detail page, and its
settings live on a page of their own under Customize rather than in a connector's config form.
What it keeps is the checkbox and the tool plumbing.

## 2. Decisions

| # | Decision | Why |
|---|----------|-----|
| A1 | **A sub agent is a chat row with a `parent_turn_id`**, hidden from the sidebar. | It buys the runner, the event stream, persistence, usage accounting and a readable transcript for one column. An in-memory run would be less code today and no transparency ever, and 05 is a document about transparency. |
| A2 | **The parent waits.** `run` blocks until the sub agent reports. | A turn that ends while its children run has to be woken up again later by something, and that "something" is a new machine — a resumable turn — for a gain the first version cannot measure. Several sub agents in one round still run at once, because the runner already executes a parallel-safe batch concurrently. |
| A3 | **Two kinds of type, and the library holds both.** A type may fix its instructions, or leave them to the parent — and it may do that field by field. | Olav's answer to question 1: a research agent wants strict rules written once; a coding agent is better told what to do by the model that knows what it wants done. Both are the same record with different fields open. |
| A4 | **Two built-in types ship**, one of each kind: `researcher`, which fixes everything about itself, and `agent`, which fixes nothing but its description. | One of each is what teaches the shape A3 describes; a library of five nobody asked for is five things to maintain and a longer tool description on every turn. The plan said one, and building it showed that one cannot demonstrate a distinction between two kinds. |
| A5 | **Permission requests from a sub agent carry the parent's chat and turn id**, so the card appears in the user's own chat, labelled with the agent that asked. A setting switches this to **the guard answers instead**. | `Interaction` already carries both ids, so the routing is nearly free. Olav will run with the guard; the default is to ask, because silently removing the user from decisions about their own machine is not a default. |
| A6 | **The parent's chat shows almost nothing while a sub agent runs**: the parent may say it delegated, and one quiet line says what is being waited for. | Olav's answer to question 11. A sub agent's steps in the parent transcript is a log file pasted into a conversation. |
| A7 | **The agent tree is a modal**, opened from that line: the parent and its sub agents as a tree, each node openable to its own read-only transcript. | One place to look, reachable without leaving the chat, and it does not compete with the right pane, which belongs to artifacts and files. |
| A8 | **You cannot talk to a sub agent.** The modal has no composer. The single exception is a permission card, and that is answered in the parent chat, not in the modal. | A sub agent's whole contract is one task in, one report out. An answer typed into the middle of it would have nowhere to go in the parent's transcript. |
| A9 | **One level in v1.** A sub agent may not start sub agents. | A tree that grows on its own spends money nobody is watching. Deferred to after launch, not rejected. |
| A10 | **The model is the parent's model unless the user has written rules**, and where rules exist the parent chooses between them. | Olav's answer to question 5: he wants to name models and say when each is for, then let the model pick. This is the media connector's pattern — the user's settings decide the menu, the model reads it and picks (03 §5). |
| A11 | **A sub agent hands back text**, never an artifact, never a file it leaves in the chat. | An artifact appearing from a conversation the user cannot see is unexplainable. The parent can always make one from what it was told. |
| A12 | **Transcripts are kept forever by default**, with a retention setting and a line in Data & privacy. | They are the bulky part of a long code session, and 06 §8 already sweeps blobs; this is the same bargain made visible. |
| A13 | **Off in incognito by default**, switchable. | Incognito exists so a conversation leaves no rows. Sub agents write rows. |
| A14 | **On by default on the Code surface**, a checkbox in Chat. | 16 §8 turns three connectors on when a code session opens; this joins them. A chat gets it when the user asks. |

## 3. The library

An **agent type** is a record. The built-in `researcher` ships as one and is editable like the
rest (a copy is made on first edit; **Reset** puts the original back).

| Field | What it is | Fixed or open |
|-------|-----------|---------------|
| `id` | slug; what the parent names in the call | fixed |
| `name` | what the user sees | fixed |
| `description` | one line, read by the **parent** when choosing a type | fixed |
| `instructions` | the system prompt fragment the sub agent runs under | fixed **or open** |
| `model` | inherit the parent's, a named one, or "let the parent choose from my rules" (§5) | fixed or open |
| `connectors` | which namespaces the sub agent may use; `inherit` means the parent chat's | fixed or open |
| `mode` | permission mode and guard for the sub agent's own calls | fixed or open |
| `write` | whether it may change files in the session's roots, or only read | fixed or open |
| `memory`, `skills` | whether it may read memories and load skills (12) | fixed or open |
| `enabled` | off types are not offered to the model | — |

"Open" means the parent may set it in the call. Every open field appears as an optional argument
on the tool; naming one that the chosen type fixes is refused with a message saying which type
fixed it, rather than silently ignored — an argument that is quietly dropped is how the media
connector taught a model to keep sending one (03 §5).

`researcher` ships with everything fixed: the web connector, read-only, Auto with the guard, and
its own instructions about reading pages before answering and quoting what it read. `agent` ships
with `instructions`, `connectors`, `write` and `model` all open and no opinion of its own beyond
"you are a sub agent; report, do not ask" — it is the one a coding task is given.

## 4. The tool

One tool, `subagents__run`, at `RiskTier::App`, `parallel_safe = true`:

```
subagents__run {
  agent: "researcher" | "<other enabled types>",
  task: string,                  // what to do, in the parent's words
  instructions?: string,         // only where a type leaves them open
  model?: "<rule ids>",          // only where the user has written rules (§5)
  connectors?: string[],         // only where a type leaves them open
  write?: boolean                // only where a type leaves it open
}
```

The optional properties exist only when at least one enabled type opens them, the way `media`'s
`model` argument disappears under the "always" rule. The `agent` enum and its per-type notes are
rebuilt from the library on every turn, because `Connector::tools()` is asked per turn.

One tool rather than one per type: the library can reach a dozen entries, and a dozen tools
crowd out the connectors in every request of every turn. The cost is that choosing a type is
choosing an enum value rather than a tool name, which models do well.

The result is one text block: the sub agent's final message, its type, its model, its token
count and how long it took. Nothing else crosses back.

## 5. Models: the user's rules

Settings hold a list the user writes:

| Model | When to use it |
|-------|----------------|
| `anthropic/claude-sonnet-5` | research and reading long documents |
| `openrouter/deepseek/deepseek-v4-flash` | anything short |

With the list empty, a sub agent runs the parent's model and the tool has no `model` argument.
With entries, `model` is an enum of `"same as mine"` plus each row, each option carrying its
"when to use it" text as its description, and the parent picks. The rules are *shown* to the
model, never enforced against it: a rule the app enforced would need to understand the task,
which is the thing the model is for.

## 6. Permissions

A sub agent's calls go through the same tiers, the same modes, the same guard and the same
guardrail floor as anything else (04). Two settings decide what happens at a card:

- **Ask me** (default). The `Interaction` carries the parent's `chat_id` and `turn_id`, so the
  card renders in the user's chat with a line naming the agent and its task. Answering it
  releases the sub agent. The turn's own cancel still cancels everything under it.
- **The guard answers** (Olav's choice). The sub agent runs in Auto with the judge, whatever the
  parent's mode is. Decisions appear in Guard & guardrails like every other judge decision, so
  "what did it allow while I was not looking" has an answer.

A type may also fix its own mode, which is how `researcher` is read-only in a chat running in
Auto: the type is the narrower of the two, never the wider. **A sub agent can never hold more
permission than its parent chat.**

## 7. What the user sees

**In the chat.** One line where the tool call is: *"Waiting for 2 sub agents"* while they run,
then *"2 sub agents reported"*, with the elapsed time and the tokens they spent. The parent's own
words above it are whatever the model chose to say. Clicking the line opens the tree.

**The tree modal.** The parent turn at the root, each sub agent under it with its type, model,
status, duration and tokens, updating live. Clicking a node opens that agent's transcript inside
the modal: the same turn view the chat uses, read-only, no composer. The parent node opens the
chat's own turn, which is how the tree reads as one thing rather than a list of orphans.

**The footer.** A turn that used sub agents shows its own tokens and the total under it, because
a turn that cost eight times what its own transcript explains is the first thing a user will want
explained.

## 8. Data model

```sql
-- migration 0016
CREATE TABLE agent_types (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL,
  instructions TEXT NOT NULL, model_json TEXT NOT NULL, connectors_json TEXT NOT NULL,
  mode TEXT, guard INTEGER, write_files INTEGER NOT NULL, memory INTEGER NOT NULL,
  skills INTEGER NOT NULL, open_json TEXT NOT NULL,   -- which fields the parent may set
  builtin INTEGER NOT NULL, enabled INTEGER NOT NULL,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
ALTER TABLE chats ADD COLUMN parent_turn_id TEXT REFERENCES turns(id) ON DELETE CASCADE;
ALTER TABLE chats ADD COLUMN agent_type TEXT;
CREATE INDEX chats_parent ON chats(parent_turn_id);
```

Every list of chats filters `parent_turn_id IS NULL`: the sidebar, search, export, the palette.
The cascade is deliberate — deleting a chat takes its sub agents' transcripts with it, which is
what a user deleting a conversation means.

`settings.subagents` holds the rest: `permission` (`ask` | `guard`), `max_concurrent` (3),
`max_per_turn` (10), `model_rules`, `in_incognito` (false), `keep_days` (0 = forever).

## 9. Limits, cancellation and cost

- **3 at once, 10 per turn**, both settings. Over the limit the call returns a refusal naming the
  limit, which the parent can act on; queueing would turn a blocking call into an unbounded one.
- **Tool rounds** per sub agent are the parent's own limit (`advanced.max_tool_rounds`).
- **Cancel is a tree.** Stopping the parent turn cancels every sub agent under it, and a sub
  agent that fails returns its failure as the tool result rather than failing the parent's turn.
- **Depth is checked in code, not by convention**: a chat with a `parent_turn_id` does not get the
  `subagents` namespace in its tool list at all (A9).

## 10. Where it lands

Three phases, each one a thing Olav can run.

**Phase A — it runs. Built 2026-09-17.** Migration 0016, the `subagents` connector and its one
tool, the two shipped types, sub-agent chats and their prompts, permission routing, the limits,
cancellation, `catalog.hidden`, attached by default on Code and a checkbox in Chat. Five things
came out of building it that the plan above did not say:

- **The connector lives in `gantry-agent`, not in a crate of its own.** Running a sub agent *is*
  running a turn, and everything a turn needs — the providers, the settings, the guard, the
  interaction registry, the notifier — is already assembled in the turn manager. A separate
  crate would have had to be handed all of it a second time. Its manifest names `gantry-agent`
  as its crate, which is why the schema's crate pattern is now `^gantry-[a-z0-9-]+$`.
- **The turn manager and the connector point at each other**, so the handle is an
  `Arc<RwLock<Weak<TurnManager>>>` filled in by `startup.rs` after the manager exists. Before
  that moment the tool answers that sub agents are not available, which is true.
- **Cancelling had to survive the future being dropped.** `runner::execute` cancels a call by
  dropping its future, so the `select!` inside `run_sub_agent` is never polled again and the
  code that started the sub agent gets no chance to stop it. A `StopOnDrop` guard does it on
  every path — cancelled, dropped, panicked. Without it a stopped turn left a model running,
  spending money on an answer nobody would read.
- **Read-only is a filter on the tool set**, not a refusal at the call: `ToolSet::assemble` takes
  a `read_only` flag and drops every tool above `RiskTier::Read`, the way Plan mode hides the
  tools it would deny.
- **The limits went where they could be counted.** Per-turn totals refuse (the model can act on
  a refusal); concurrency waits on a semaphore (the model has already decided the work, and the
  limit is about the machine).

The UI is the ordinary tool-call row — `agent=researcher task=…` — until phase C.

**Phase B — you can shape it. Built 2026-09-17.** Customize → **Sub agents**: the library list and
its editor with a *the caller decides* box beside every openable field, the model rules table, and
the switches — ask me or the guard, the two limits, incognito, retention. Five commands
(`list_agent_types`, `save_agent_type`, `delete_agent_type`, `reset_agent_type`,
`set_agent_type_enabled`) and an `AgentTypesChanged` event; the settings beside them go through
`update_settings` like everything else, because a page that saved through two mechanisms would be
a page with two ways to fail.

Two things changed from phase A while building it:

- **The built-in types moved out of migration 0016 and into `subagents::library`**, seeded at
  startup for whatever the table is missing. **Reset** has to know what the original said, and a
  second copy of the same paragraph inside a SQL file is a second copy to keep in step. Seeding
  fills what is *missing*, never what is there, so an edited `researcher` survives every release.
- **`keep_days` actually sweeps.** It was going to be phase C, but a number in a settings form
  that does nothing is worse than no number: `ChatBook::sweep_sub_agents` runs at startup beside
  the incognito sweep, and zero — the default — means forever.

**Phase C — you can see it. Built 2026-09-17.** The parent's one line where the tool call was —
*"Waiting for 2 sub agents"*, then *"1 sub agent reported · 34 s · 18,400 tokens"* — the tree it
opens, each node's read-only transcript inside the modal, the footer's roll-up, and the
Data & privacy row that says the transcripts exist. One command (`list_sub_agents`) and one DTO
(`SubAgentNode`); everything else is projection.

Four things came out of building it:

- **The line is folded in the view model, not in the backend.** Consecutive `subagents__run`
  calls become one `subagents` activity item holding one run each, because "Waiting for 3 sub
  agents" is the sentence — three rows each saying it about one agent is a log file. The fold
  turned up a real bug in the projection: the pass that adds calls the stream announced but the
  transcript has not caught up with matched on the row's id, and a row that stands for three
  calls carries one id, so the second and third were added a second time. It now answers for
  every call in it.
- **The row does not fold away with the tool work.** `TurnSteps` keeps it beside the context and
  notice rows, always visible: it is the one line the parent's chat says about a conversation the
  user is not having (A6), and behind "used a tool ×3" it would say nothing at all.
- **Live is a one-second refetch, not a new event.** A sub agent's events reach the database and
  no channel — nobody is watching its conversation — so the tree polls while anything under the
  turn is running and stops when nothing is. An event would have been a second delivery path for
  a modal that is open for a minute at a time.
- **`chat_count` in Data & privacy stopped counting sub agents.** They are chat rows (A1), and
  "412 chats" on a machine with forty of them is a number about the schema. Sub-agent transcripts
  are counted on a row of their own, which is where the retention is explained.

## 11. Not in v1

Sub agents starting sub agents (A9). A parent that keeps working while its children run (A2).
Sub agents that talk to each other. A sub agent that writes an artifact (A11). Sharing or
importing agent types — the skill library's import/export exists and the same shape would work,
but nobody has asked for it yet.
