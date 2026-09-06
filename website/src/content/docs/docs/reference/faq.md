---
title: FAQ
description: The questions people ask before installing.
sidebar: { order: 6 }
---

## Is there an account?

No. You need an API key from a model provider, and that is all.

## Does anything get sent to Gantry?

No. There is no Gantry server. Requests go from your machine to the provider or service you configured. The website makes two optional requests to GitHub, for the star count and the latest release.

## Which models can I use?

Anthropic, OpenAI, Google, xAI and OpenRouter models with your keys, local models through Ollama, and any OpenAI-compatible endpoint.

## Can I use it offline?

The app runs offline; chats need a model, so with Ollama you can work with no network at all.

## Is it open source?

Source-available under the [Functional Source License](/license/), which converts each release to Apache 2.0 after two years. You can read, build, modify and share it.

## How do I request a connector?

[Open a connector request](https://github.com/oljodev/gantry/issues/new?template=connector-request.yml) on GitHub. The app will have the same link in its Connectors window.
