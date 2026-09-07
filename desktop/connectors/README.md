# Connectors

Every connector Gantry knows about lives here, one folder each, first-party or not. The folder
is the unit of packaging and the catalog is built from it at compile time. The full contract is
`docs/plan/03-connector-system.md` §2–§3; this file is the short version.

## The folder

```
desktop/connectors/<id>/
  manifest.json     required · id, name, runtime, auth, risk, tools (schema: desktop/schemas/connector-manifest.schema.json)
  icon.svg          required · 24×24 viewBox, single colour, no text
  README.md         required · what it does, what it needs, what it can reach
  Cargo.toml + src/ native connectors only · a crate named gantry-connector-<id> implementing `Connector`
  ui/index.tsx      optional · a custom settings panel, referenced by `settings_ui`
```

## Rules

- `id` equals the folder name, is immutable, and prefixes every tool name (`github__create_issue`).
- Three runtimes: `native` (a Rust crate in this folder), `mcp-stdio` (a process on a runtime the
  user installs: Node, Python/uv, Docker), `mcp-remote` (Streamable HTTP). Nothing is bundled
  as a sidecar.
- Nothing is installed without an explicit Install action, first-party connectors included.
- Every tool carries a risk tier. MCP servers that discover tools at runtime set
  `tools_generated: true` and may map tiers through `tool_overrides`.
- Third-party names and logos belong to their owners; a bundled manifest describes a server it
  does not ship.

## Adding one

Native: create the folder with the four required files, add the crate to `[workspace].members`
in the root `Cargo.toml` and to `gantry-connectors`' dependencies. MCP: the three required files
are enough; `gantry-connectors`' `build.rs` embeds every manifest it finds and validation runs in
CI (`cargo xtask validate-connectors`).
