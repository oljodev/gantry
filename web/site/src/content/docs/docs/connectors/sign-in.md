---
title: Signing in
description: OAuth, API keys and connection strings, and where each one is stored.
sidebar: { order: 3 }
---

Connectors sign in three ways.

## OAuth

Services like GitHub, Notion, Slack or Google open your browser. You sign in there, the service redirects back to Gantry on a local address, and the token is handed to the app. Gantry identifies itself to the service with a public client document at `id.oljo.dev`; there is no Gantry server in the exchange. Tokens are encrypted with your master key and refreshed automatically.

## API keys

Some services, mostly search and data APIs, use a key you create in their console. Paste it when installing; it is stored encrypted like everything else.

## Connection strings

Databases take a connection string. It is stored encrypted, used only by the connector's process, and never shown again in full.

## Revoking

Removing a connector deletes its credentials from Gantry. Revoking access on the service's side works too; Gantry will ask you to sign in again the next time the connector is used.
