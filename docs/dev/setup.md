# Developer setup

Gantry is a Tauri 2 app: a Rust backend in `src-tauri/` and `crates/`, a React frontend in
`src/`. One Cargo workspace, one pnpm package.

## Prerequisites

| Everywhere | |
|------------|--|
| Rust 1.93 | `rustup` picks it up from `rust-toolchain.toml` |
| Node 22 or newer | `.node-version` says 22 |
| pnpm 10 | `corepack enable` then `corepack prepare pnpm@10.34.5 --activate` |

**macOS**: Xcode Command Line Tools (`xcode-select --install`).

**Windows**: Visual Studio Build Tools with the "Desktop development with C++" workload, and the
WebView2 runtime (present on Windows 10/11).

**Linux (Debian/Ubuntu)**:

```sh
sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf xdg-utils
```

Other distributions: see the Tauri prerequisites page for the package names.

## First run

```sh
pnpm install          # frontend dependencies (installs the Tauri CLI too)
pnpm fonts            # copies Inter and JetBrains Mono into src/assets/fonts/
pnpm tauri dev        # builds the Rust side, starts Vite on :1420, opens the window
```

The first Rust build takes a few minutes; later ones are incremental.

## Everyday commands

| Command | What |
|---------|------|
| `pnpm tauri dev` | the app with hot reload |
| `pnpm dev` | the frontend alone in a browser on http://localhost:1420 (no backend; Tauri calls are skipped) |
| `cargo test --workspace` | Rust tests, including the bindings drift check |
| `cargo clippy --workspace --all-targets -- -D warnings` | lints |
| `pnpm typecheck`, `pnpm lint`, `pnpm test`, `pnpm format` | frontend checks |
| `cargo xtask gen-bindings` | regenerate `src/bindings.ts` after changing a command |
| `pnpm tauri build` | an installable bundle in `src-tauri/target/release/bundle/` |

## Bindings

Commands are declared once in Rust (`src-tauri/src/commands/`) and collected in
`src-tauri/src/lib.rs`. `tauri-specta` writes `src/bindings.ts` on every debug start, and the
`gen_bindings` test fails when the committed file differs from what the Rust side would generate.
After adding or changing a command: `cargo xtask gen-bindings`, then commit the result.

## Where the app keeps its data

Tauri's app data directory under the identifier `dev.oljo.gantry`:

| OS | Data | Logs |
|----|------|------|
| macOS | `~/Library/Application Support/dev.oljo.gantry` | `~/Library/Logs/dev.oljo.gantry` |
| Windows | `%APPDATA%\dev.oljo.gantry` | `%LOCALAPPDATA%\dev.oljo.gantry\logs` |
| Linux | `~/.local/share/dev.oljo.gantry` | `~/.local/share/dev.oljo.gantry/logs` |

Settings → About shows the exact paths.

## Design rules

UI work follows `docs/plan/15-app-design.md`. Every colour, size, radius and duration comes from
`src/styles/tokens.css`; components never carry their own values.
