---
title: Install a connector
description: From the catalog to a working tool in three steps.
sidebar: { order: 2 }
---

1. Open **Connectors** in the sidebar. The **Catalog** tab lists what Gantry knows about; **Installed** lists what you have.
2. Choose a connector and read what it exposes: its tools, their risk tiers, and how it signs in.
3. Choose **Install**. Depending on the connector, Gantry then opens your browser to sign in, asks for an API key or a connection string, or simply finishes.

## Local runtimes

MCP servers that run locally need a runtime, usually Node or Python. Before installing one, Gantry checks for the runtime and tells you what is missing and where to get it. It never installs a runtime for you.

## Attaching to chats

An installed connector can be attached to a single chat, or made available to every chat in a project. Its tools appear to the model from the next turn.

## Removing

**Remove** on the Installed tab stops the server if it is running, deletes its tokens and settings, and forgets it. Chats that used it keep their transcripts.
