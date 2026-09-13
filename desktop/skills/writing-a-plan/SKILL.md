---
name: writing-a-plan
description: Turn a vague piece of work into a plan somebody can act on: the decision and why, the scope as a short list of concrete deliverables, the order of the work, what will be verified, and what was deliberately left out. Use when asked for a plan, a design, an approach, an implementation strategy or a proposal, or before starting a change large enough to need one.
license: FSL-1.1-ALv2
metadata:
  gantry-triggers: "plan, design doc, proposal, approach, strategy, roadmap, break this down, how should we"
  gantry-always: "false"
  gantry-version: "2"
  author: Gantry
---

# Writing a plan

## Where it goes

**A plan is an artifact, not a reply.** Call `gantry__create_artifact` with type `markdown`,
titled for the work rather than for the document — "Chess engine in Rust", not "Plan for a chess
engine" — and put the whole plan in it. In the chat, say in a sentence or two what you decided
and what the reader should look at first; do not repeat the plan underneath it.

A plan is read more than once, gets argued with, and gets edited. That is the definition of an
artifact: it opens beside the conversation, it keeps its versions, and it is still there when
the chat has scrolled past. A plan pasted into the transcript has none of that and has to be
re-read from the top every time somebody wants the third deliverable.

The one exception is a plan of three or four lines, which is an answer wearing a plan's clothes.
Say it in the chat.

## When to use

The work is big enough that the first decision is not obvious, or somebody other than you will
do part of it. Not for a change you can simply make — writing a plan for a ten-minute edit is a
way of not making it.

## The shape

**Context.** Two or three sentences: what exists now, what is wrong with it, what changed to
make this worth doing. Name the constraints that actually bind — a deadline, a dependency, a
decision already made elsewhere.

**The decision.** One paragraph that states the approach and the reason, before any detail.
If there were real alternatives, name the strongest one and say in a sentence why it lost. A
plan that presents only the chosen path hides the part a reader most needs to check.

**Scope.** A numbered list of deliverables, each concrete enough to be finished or not finished.
"Improve error handling" is not a deliverable; "every provider error carries a `retryable` flag
and the UI shows a Retry button for the ones that are" is.

**Order.** What is built first, and why that order — usually: what everything else depends on,
then what proves the design works, then the rest. Say what can be done in parallel.

**Verification.** For each part, what says it works: a test, a command to run, a thing to click.
Written before the work, so it cannot be back-fitted to what happened to get built.

**Out of scope.** What you considered and are not doing, with the reason. This is the section
that prevents the plan being relitigated halfway through, and the one most often missing.

## Rules

1. **Decide.** A plan that lists options without choosing is a survey. Recommend, and say what
   would change your mind.
2. **Be concrete enough to be wrong.** Name files, tables, commands, function names. A plan that
   cannot be contradicted cannot be reviewed.
3. **Size it honestly.** If a part is uncertain, say so and say what would settle it — usually a
   spike with a fixed time budget, whose output is an answer rather than code.
4. **Write for the person doing the work**, who may be you in three weeks with none of today's
   context.
5. **Keep it short.** A plan longer than the change it describes has stopped being useful.

## Pitfalls

- A scope list that grows during writing. Anything discovered mid-plan goes in Out of scope
  unless it blocks the first deliverable.
- Verification written as "test it". Name the input and the expected output.
- Confident estimates on the part nobody has tried. Mark it, do not smooth it over.
