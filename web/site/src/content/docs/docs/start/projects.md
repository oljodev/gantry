---
title: Projects
description: Group chats with the folders, files and instructions they share.
sidebar: { order: 4 }
---

A project is a home for related chats. It holds:

- **Folders** the agent may work in. A chat in the project can read, search and (depending on the permission mode) change files in them.
- **Knowledge files**: documents you want every chat in the project to know about, sent as context.
- **Instructions**: a short text added to the system prompt of every chat in the project, for conventions, tone or rules.
- **Connectors** that are available to every chat in the project without attaching them one by one.
- **Artifacts** produced by its chats, listed in one place.

## Creating a project

Choose **New project** in the sidebar, give it a name, and add at least one folder if you want the agent to code in it. Chats created inside the project inherit its settings; a chat can still override the model and the permission mode.

## Instructions and the system prompt

Gantry's system prompt has a fixed core that is never replaced, plus additive layers: your global instructions, the project's instructions, and the chat's own. Each layer has a size limit so the prompt stays predictable. Pinned [skills](/docs/reference/skills/) are added the same way.
