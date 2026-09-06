---
title: Connectors
description: What a connector is, which ones are built in, and how MCP servers fit.
sidebar: { order: 1 }
---

A connector gives the agent tools. Four are built into Gantry; the rest are [Model Context Protocol](https://modelcontextprotocol.io) servers.

## Built in

- **Filesystem**: read, search and write files in the folders you attach.
- **Code editor**: targeted edits shown as diffs.
- **Shell**: commands with streaming output.
- **Web**: fetch a page and read it as text.

They are part of the app, written in Rust, and need nothing installed.

## MCP servers

Everything else, from GitHub to Postgres to Slack, is an MCP server. Some run as a local process on the Node or Python you already have; some are hosted by the service and reached over HTTPS. Gantry's catalog lists the ones it has checked, with the tools each exposes and how it signs in. The full list is on the [connectors page](/connectors/).

## Nothing installs itself

The catalog ships with each release, and every connector waits for an explicit **Install**. The agent can suggest a connector when a task would need one; only you can add it.

## The same rules for every tool

Each tool a server exposes is mapped to a [risk tier](/docs/start/permissions/), so the chat's permission mode applies to a connector's tools exactly as it does to the built-in ones.
