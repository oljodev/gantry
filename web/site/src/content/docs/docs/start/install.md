---
title: Install
description: Get Gantry onto Windows or Linux, or build it from source.
sidebar: { order: 1 }
---

Gantry runs on Windows and Linux. Downloads come from the [download page](/download/), which always points at the latest release on GitHub; the release page itself also has checksums for every file.

## Windows

1. Download `Gantry-Windows-x64.exe` and run it.
2. The installer is not signed yet, so Windows SmartScreen stops it the first time with "Windows protected your PC". Choose **More info**, then **Run anyway**.
3. The installer puts Gantry in your user profile; no administrator rights are needed.

Windows 10 or later, 64-bit. Gantry draws its window with Microsoft's WebView2, which Windows 11 already has; on a Windows 10 machine without it, the installer downloads it from Microsoft.

## Linux

The AppImage runs on most distributions as it is:

1. Download `Gantry-Linux-x86_64.AppImage`.
2. Make it executable (`chmod +x Gantry-Linux-x86_64.AppImage`) and run it.

The AppImage carries its own copy of WebKitGTK and the libraries around it, so it does not depend on what your distribution has installed. If it says FUSE is missing, install your distribution's FUSE 2 package (`libfuse2t64` on Ubuntu 24.04, `libfuse2` on older releases) or start it with `--appimage-extract-and-run`.

The [release page](https://github.com/oljodev/gantry/releases/latest) also has packages that use the system's WebKitGTK 4.1 instead, which makes them much smaller:

- Debian, Ubuntu and relatives: `sudo apt install ./Gantry-Linux-x86_64.deb`
- Fedora, openSUSE and relatives: `sudo dnf install ./Gantry-Linux-x86_64.rpm`

x86_64 only for now.

## macOS

There is no macOS download yet. An app from the internet has to be signed and notarised by Apple before a Mac will open it without a fight, and that is not set up. It is on the list after Windows and Linux.

## Build from source

Gantry is a [Tauri](https://tauri.app) app: Rust for the backend, React for the interface. Install [Tauri's prerequisites](https://tauri.app/start/prerequisites/) for your system (on Linux, that includes the WebKitGTK development packages), then with a Rust toolchain, Node 22 and pnpm:

```sh
git clone https://github.com/oljodev/gantry
cd gantry
pnpm install && pnpm fonts
pnpm tauri build
```

The build lands in `target/release/bundle/`.

## What Gantry installs on your machine

The app, and nothing else. Connectors are added later, one at a time, from inside the app. Third-party MCP servers that run locally use the Node or Python you already have; Gantry checks and tells you what is missing before it installs one.
