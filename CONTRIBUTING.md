# Contributing

Gantry is pre-release and moving fast against a written plan. Read `docs/plan/README.md` first;
it says what is decided and why. Issues and discussions are the channels; there is no mailing
list.

## Building

See `docs/dev/setup.md` for prerequisites per OS. In short:

```sh
pnpm install && pnpm fonts       # frontend dependencies and the bundled fonts
pnpm tauri dev                   # the app, with hot reload
cargo test --workspace           # Rust tests
pnpm typecheck && pnpm lint      # frontend checks
cargo xtask check-bindings       # src/bindings.ts must match the Rust commands
```

## Changes

- Small, coherent commits with a subject line that says what changed and why.
- Rust: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, tests next to the
  code. Frontend: Prettier, ESLint, Vitest.
- UI work follows `docs/plan/15-app-design.md`. Colours, sizes, radii and durations come from
  `src/styles/tokens.css`; a component never carries its own.
- A change that contradicts a plan document updates that document in the same commit.
- Bindings: after adding or changing a command, run `cargo xtask gen-bindings` and commit
  `src/bindings.ts`.

## Sign-off

By contributing you agree to the [Developer Certificate of Origin](https://developercertificate.org/).
Add `Signed-off-by: Your Name <you@example.com>` to each commit (`git commit -s`). There is no
contributor licence agreement. Contributions are licensed under the repository licence
(`FSL-1.1-ALv2`, see `LICENSING.md`); the licensor may relicense future versions, and the FSL's
future-licence grant already converts every version to Apache 2.0 two years after release.

## Security

Do not open public issues for vulnerabilities; see `SECURITY.md`.
