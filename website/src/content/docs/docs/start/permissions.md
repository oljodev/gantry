---
title: Permission modes
description: Manual, Auto-edit, Plan and Auto, the risk tiers behind them, and what the guard does.
sidebar: { order: 6 }
---

The permission mode is a setting on each chat. It decides which tool calls run on their own and which wait for you.

## The four modes

| Mode | What runs on its own | What asks |
|------|----------------------|-----------|
| **Manual** | nothing | everything, even reads |
| **Auto-edit** | reads, and edits inside the project's folders | commands, external writes, anything destructive |
| **Plan** | reads and searches | nothing else is allowed; the agent produces a plan for you to approve |
| **Auto** | everything, with the guard reviewing risky calls | destructive calls, always |

## Risk tiers

Every tool is mapped to a tier: `read`, `write` (inside your folders), `write_external` (a pull request, a message, a database row), `execute` (a command), `destructive` (delete, drop, force-push) and `app` (Gantry's own state: memory, artifacts, skills). The mode decides what each tier does, so a Slack message follows the same rules as a shell command.

## The guard

In Auto mode, **Guard** is on by default. For each risky call a fast model from your own provider is asked one question: does this call fit the task the user set? It answers allow or deny with a reason; denied calls are shown in the feed and the agent is told. If the guard itself fails, the call falls back to asking you. You can turn Guard off per chat, in which case only destructive calls ask.

## Answering a prompt

A prompt shows the tool, the exact call and, for edits, the diff. You can **allow once**, **allow for this chat**, or **deny** with an optional note the model will read.
