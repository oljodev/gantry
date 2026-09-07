---
title: Your first chat
description: Models, the composer, attachments and artifacts.
sidebar: { order: 3 }
---

A chat in Gantry is a conversation with one model at a time, with tools available when you want them.

## Starting a chat

Press the new-chat button in the sidebar or use the keyboard shortcut. The composer shows the current model and permission mode as chips; click either to change it for this chat.

## Choosing a model

Every provider you added contributes its models. Switching model mid-chat is allowed; the conversation so far is sent to the new model. Reasoning models show their thinking in a collapsible block when the provider returns it.

## Attachments

Drop files onto the composer to attach them. Text-like files are sent as text; images are sent to models that accept them. Attachments are stored in Gantry's blob folder and referenced from the chat, so the database stays small.

## Artifacts

When a model produces a document, a piece of code, a diagram or a small interactive app, Gantry shows it as an artifact beside the chat rather than inline. Artifacts are versioned; every edit is a new version and you can step back. Interactive artifacts run in a sandbox with no access to your files or keys.

## The transcript

Everything in a chat is kept locally: messages, tool calls with their inputs and results, permission decisions and artifacts. Full-text search across all chats is in the sidebar.
