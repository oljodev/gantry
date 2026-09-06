---
title: The agent
description: How the agent reads, edits and runs things, and how to follow along in the activity feed.
sidebar: { order: 5 }
---

Any chat becomes an agent session the moment the model uses a tool. Tools come from the built-in connectors (filesystem, code editor, shell, web) and from the MCP connectors you installed.

## What it can do

- **Read and search** files in the project's folders.
- **Edit** files with targeted replacements. Each edit is shown as a diff in the feed, before and after, and can be reverted from there.
- **Run commands** in your shell, with output streaming into the feed as it happens.
- **Fetch pages** from the web and read them as text.
- **Use connectors**: open a pull request, query a database, search your documents, post a message.

## The activity feed

Every tool call is a row: which tool, what it was asked to do, and what came back. Click a row for the full input and output, the diff of an edit, or the complete output of a command. Permission decisions, including the guard's reasoning in Auto mode, are rows too. The feed is part of the transcript and is kept with the chat.

## Stopping and steering

Press Stop at any time; the current tool call finishes or is cancelled, and the model gets to see what happened. You can then reply, change the permission mode, or attach another folder and continue.

## Where it cannot go

The agent only reaches folders you attached, and only reaches services through connectors you installed. Commands run with your user's permissions inside those folders by default. See [Permission modes](/docs/start/permissions/) for the controls.
