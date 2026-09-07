---
title: Install
description: Get Gantry onto macOS, Windows or Linux, or build it from source.
sidebar: { order: 1 }
---

Gantry is one desktop app for the three desktops. Downloads come from the [download page](/download/), which always points at the latest release on GitHub.

## macOS

1. Download `Gantry-macOS.dmg` and open it.
2. Drag Gantry into Applications and launch it.
3. macOS asks once whether to open an app downloaded from the internet. Builds are signed and notarised.

Apple silicon and Intel Macs run the same build. macOS 10.15 or later.

## Windows

1. Download `Gantry-Windows-x64.exe` and run it.
2. The installer puts Gantry in your user profile; no administrator rights are needed.

Windows 10 or later, 64-bit.

## Linux

1. Download `Gantry-Linux-x86_64.AppImage`.
2. Make it executable (`chmod +x Gantry-Linux-x86_64.AppImage`) and run it.

Gantry uses your system's WebKitGTK 4.1, which ships with Ubuntu 22.04 and later, Fedora 36 and later, and their relatives.

## Build from source

Gantry is a [Tauri](https://tauri.app) app: Rust for the backend, React for the interface. With a Rust toolchain, Node 22 and pnpm installed:

```sh
git clone https://github.com/oljodev/gantry
cd gantry
pnpm install
pnpm tauri build
```

The build lands in `src-tauri/target/release/bundle/`.

## What Gantry installs on your machine

The app, and nothing else. Connectors are added later, one at a time, from inside the app. Third-party MCP servers that run locally use the Node or Python you already have; Gantry checks and tells you what is missing before it installs one.
