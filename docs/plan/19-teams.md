# 19 — Teams

A **team** is a pipeline of agents that runs to a finish: research hands its findings to a
writer, a planner splits a job between two coders and a reviewer reads what they produced. The
user names a goal; a sequence of sub agents does the work; one report comes back.

Sub agents (18) settled what one delegated conversation is. This document settles what several
of them in a row are: what a team is as a record, who decides its shape, how one stage's output
becomes the next one's input, where it lives in the app, and what lands in which phase. It was
designed with Olav on 2026-09-21, answering four questions; his answers are the decisions below.

---

## 1. What a team is, and what it is not

The obvious design for "agents that work together" is a blackboard: shared state every member
reads and writes, and messages passing between them. It was proposed and rejected here, because
it is not what the work looks like.

The work looks like a **sequence**. An idea is researched, then written. A change is planned,
then coded, then tested, then reviewed, then published. Where two agents genuinely run at once
they are not talking to each other — they are two coders who each got a different task from the
same planner, and each reports back to whoever assigned it.

So the primitive is not a shared surface. It is a **handoff**: what a stage produced is what the
next stage is given. And the only communication is upward, to whoever handed out the work.

This is worth stating plainly because it decides how much has to be built. A blackboard needs
new state, new tools, a concurrency story and a way to show it. A pipeline needs an ordered list
and the machinery sub agents already have.

## 2. Decisions

| # | Decision | Why |
|---|----------|-----|
| A1 | **A team is an ordered list of stages.** A stage holds one or more members. Members inside a stage run at once; stages run in order. | Olav's answer to question 3. It is the shape of both his examples — research → script, and plan → code → test → review → publish — and it is the shape sub agents already execute: a parallel batch, then the next round. |
| A2 | **The handoff is the report.** Stage *n*'s reports are stage *n+1*'s input, appended to its task as context. | A sub agent's whole output is already one report (18 A11). Making that the transport means no new state, no new tool and nothing to keep in step: the thing that crosses between stages is the thing that already crossed. |
| A3 | **The runner owns the sequence; no member starts another member.** | The load-bearing decision. Olav's planner hands two coders a task each — but it *produces* the assignments, it does not *execute* them. The runner reads a planning stage's output and starts the next stage from it. This keeps 18 A9 (one level) exactly as it is, and a team of nine agents is still a tree one deep. |
| A4 | **A fan-out stage takes its tasks from the stage before it.** Where a stage is marked `fan_out`, its predecessor returns a list of tasks and the runner starts one member per task, up to `max_members`. | This is what "to kode agenter som jobber hver for seg, som har fått hver sin oppgave fra planleggeren" requires, and it is the only place the pipeline's width is not known in advance. A limit rather than a promise, because the number comes from a model. |
| A5 | **Two coordinators, and a team says which it uses.** Either the **caller** — the chat model calls the team as one tool and reads the final report — or a **leader**, a first stage whose job is to plan the stages after it. | Olav's answer to question 2 was both. They are the same machine: a leader is a planning stage whose output is the plan, which A3 and A4 already execute. What differs is who writes the plan, not who runs it. |
| A6 | **Open or fixed, field by field**, as agent types already are (18 A3). A team may fix its stages, or leave them to the caller; the same for each member's model, connectors and instructions. | Olav's answer to question 1. A coding team wants its five stages written once; an ad-hoc research team is better told its shape by the model that knows the job. One record, different fields open. |
| A7 | **Teams live in Customize → Teams, and a project may have its own.** A project's teams are offered in that project's chats; library teams are offered everywhere. | Olav's answer to question 4 was both. The library is where a team is made and edited; a project is where a team that only makes sense for one body of work belongs, next to that project's instructions and knowledge. |
| A8 | **The first version runs synchronously**, inside the calling turn, exactly as sub agents do today. | Olav's answer to question 3. It reuses the runner, the guard, the event stream and the tree whole, which makes the first version small. §9 is honest about what it costs. |
| A9 | **A team hands back text**, like a sub agent (18 A11). | Same reason: an artifact appearing out of a conversation the user cannot see is unexplainable, and the caller can always make one from what it was told. |
| A10 | **A member can never hold more permission than the chat that started the team**, and a stage may narrow itself further. | 18 §6, unchanged. A team is a longer delegation, not a wider one. |

## 3. The record

A **team** is a record in the same sense an agent type is, and it refers to agent types rather
than redefining them.

| Field | What it is | Fixed or open |
|-------|-----------|---------------|
| `id` | slug; what the caller names | fixed |
| `name` | what the user sees | fixed |
| `description` | one line, read by the **caller** when choosing a team | fixed |
| `goal` | what this team is for, given to every member as shared context | fixed **or open** |
| `stages` | the ordered list (§4) | fixed **or open** |
| `coordinator` | `caller` or `leader` (A5) | fixed |
| `project_id` | the project it belongs to, or none for a library team | fixed |
| `enabled` | off teams are not offered to the model | — |

A **stage** inside `stages`:

| Field | What it is |
|-------|-----------|
| `name` | what the tree shows: "research", "coding", "review" |
| `members` | one or more, each an agent type id plus per-member overrides of the fields that type leaves open |
| `fan_out` | when true, the members are made from the previous stage's task list (A4) rather than written here |
| `max_members` | the ceiling on a fan-out stage; default 3 |
| `input` | `previous` (the default), `goal_only`, or `all` — how much of what came before this stage is given |

`input` exists because a five-stage pipeline that hands every stage everything will spend the
last stage's context on the first stage's working notes. `previous` is the handoff A2 describes;
`all` is for a reviewer that has to see the whole run.

## 4. The two shipped teams

One of each coordinator, for the same reason 18 ships one agent type of each kind: one cannot
demonstrate a distinction between two.

**`research`** (coordinator: `caller`, stages fixed). Two stages — `research`, a fan-out stage of
`researcher` sub agents, and `write`, one `agent` that turns what they found into the asked-for
piece. This is the YouTube-ideas team, and it runs today's connectors with no new capability.

**`build`** (coordinator: `leader`, stages open). A leader stage that plans, then `code` as a
fan-out stage, then `test`, then `review`. It is the team a code session is given, and it is
deliberately the one whose shape the model decides, because the number of coders is a property of
the change and not of the team.

## 5. The tool

One tool, `teams__run`, at `RiskTier::App`, `parallel_safe = false` — a team is long and a second
one started beside it would double a cost nobody is watching yet.

```
teams__run {
  team: "research" | "<other enabled teams>",
  goal: string,                  // the job, in the caller's words
  stages?: [...],                // only where a team leaves them open
  members?: { ... }              // per-stage overrides, only where the members' types allow
}
```

The `team` enum and its per-team notes are rebuilt from the library on every turn, as the agent
enum is (18 §4), and a project's teams appear only in that project's chats (A7).

The result is one text block: the last stage's report, the team, how many members ran, the tokens
they spent and how long it took. Where the last stage fanned out, the reports are concatenated
under their task names, because the caller asked for one answer and a list of three is one answer
about three things.

## 6. What the user sees

**In the chat.** One line where the tool call is — *"Running research · stage 2 of 2"*, then
*"research finished · 4 members · 2 m 14 s · 61,000 tokens"*. The parent's own words above it are
whatever the model chose to say. This is 18 A6 unchanged: a team's steps in the parent transcript
is a log file pasted into a conversation.

**The tree.** The existing agent-tree modal (18 §7), with one level of grouping added: stages as
headings, members under them, each openable to its read-only transcript. A stage that has not
started yet is listed and greyed, because the one thing a person watching a five-stage run wants
to know is how much is left.

**The footer.** The turn's own tokens and the total under it, as sub agents already roll up.

## 7. Data model

```sql
-- migration 0017
CREATE TABLE teams (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL,
  goal TEXT NOT NULL, stages_json TEXT NOT NULL, open_json TEXT NOT NULL,
  coordinator TEXT NOT NULL,                       -- caller | leader
  project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
  builtin INTEGER NOT NULL, enabled INTEGER NOT NULL,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE INDEX teams_project ON teams(project_id);
ALTER TABLE chats ADD COLUMN team_id TEXT;        -- the team this member ran in
ALTER TABLE chats ADD COLUMN team_stage INTEGER;  -- its stage, for the tree's grouping
```

A member's conversation is a sub-agent chat and nothing else: `parent_turn_id`, `agent_type` and
the two columns above. There is no `team_runs` table in this version, because a run lives and
dies inside one turn (A8) and the turn already is the row. §9 says what changes when it stops
doing that.

The built-in teams are seeded from `teams::library` at startup for whatever the table is missing,
never overwriting what is there — the lesson 18 phase B learned about **Reset** and a second copy
of the same paragraph inside a SQL file.

`settings.teams` holds `max_members_per_stage` (3), `max_members_per_run` (12) and
`in_incognito` (false). Retention follows sub agents: a member's transcript is a sub-agent
transcript and the existing sweep already covers it.

## 8. Limits and cancellation

- **12 members per run, 3 per fan-out stage**, both settings. Over the limit the fan-out is
  truncated and the report says so, rather than refusing a run that is already half done.
- **Cancel is still a tree.** Stopping the parent turn cancels the running stage and every stage
  after it. The `StopOnDrop` guard 18 phase A had to add covers a member exactly as it covers a
  sub agent.
- **A failed member does not fail the run.** Its failure becomes its report, and the next stage is
  told what did not work — which is information a reviewer stage can act on.
- **A failed *stage*** — every member failed — stops the run and reports how far it got.

## 9. What A8 costs, and when it stops costing it

A synchronous team blocks its caller for as long as the whole pipeline takes. Five stages of real
work is minutes, and the chat is unusable for all of them. This is 18 A2 applied to something
five times longer, and it is the honest price of the first version being small.

It stops being the price at step 3 of the build order — the resumable turn. A team is the clearest
argument for building it: the same pipeline, started and left alone, is the feature Olav described
("agentene kan jobbe hver for seg"), and it is also what a trigger needs and what the headless
runtime needs. Nothing in this document has to change for it; A8 is replaced and the rest stands.

## 10. Where it lands

**Phase A — it runs.** Migration 0017, the `teams` record and its library, `teams__run`, the
runner executing stages in order with the handoff, fan-out from a planning stage, the two shipped
teams, both coordinators, the limits and cancellation. The UI is the ordinary tool-call row.

**Phase B — you can shape it, and so can the model.** Customize → Teams: the library list and the
stage editor with a *the caller decides* box beside every openable field; teams on a project;
and the runtime tool that lets the model write a team from chat, which is the feature this whole
document exists to make possible. The model's draft opens in the editor rather than being saved
behind the user's back: a team that appears and runs without being read is 18 A6 taken one step
too far.

**Phase C — you can see it.** The stage line in the chat, the tree grouped by stage with the
stages not yet started listed, and the footer's roll-up.

## 11. Not in v1

Members that talk to each other rather than reporting up (§1). A stage that loops back to an
earlier one — the pipeline is a line, not a graph, until something asks for it. Teams that
contain teams. A team that keeps running after its caller's turn ends (§9). Sharing or importing
teams.
