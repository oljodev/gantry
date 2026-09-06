---
title: Add a provider key
description: Gantry uses your own API keys. Here is where to get one and how it is stored.
sidebar: { order: 2 }
---

Gantry has no account and no server. To talk to a model you add an API key from the provider of your choice, and Gantry sends your requests straight to them.

## Providers

| Provider | Models | Where to get a key |
|----------|--------|--------------------|
| Anthropic | Claude | console.anthropic.com |
| OpenAI | GPT | platform.openai.com |
| Google | Gemini | aistudio.google.com |
| xAI | Grok | console.x.ai |
| OpenRouter | many, one key | openrouter.ai |
| Ollama | local models | no key; Gantry finds a running Ollama on your machine |
| Custom | any OpenAI-compatible endpoint | your own |

## Adding a key

1. Open **Settings › Providers**.
2. Choose a provider and paste the key. Gantry makes one small request to confirm it works and to list the models you can use.
3. Pick a default model. You can change the model per chat at any time.

## How keys are stored

On first run Gantry generates one random master key and stores it in your operating system's credential store: the Keychain on macOS, Credential Manager on Windows, Secret Service on Linux. Every provider key and connector token is encrypted with that master key and kept in Gantry's local database. The interface never sees a key in the clear; the backend makes the request.

Removing a key in Settings deletes it. Deleting Gantry's data folder deletes everything.

## Costs

Providers bill you per token at their own rates. Gantry shows the token usage of every turn in the chat, so you can see what a conversation costs. See [Pricing](/pricing/) for the short version.
