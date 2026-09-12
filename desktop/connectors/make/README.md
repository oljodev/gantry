# Make

Make's own server as a local process: list your scenarios, run one, read the result.

- **Runs:** `npx -y @makehq/mcp-server` on this machine, on Node 18 or later. Gantry checks for it before the install goes through and refuses with the missing runtime's name rather than failing later with an error about `npx` (03 §11 step 1), and whatever the server writes to stderr is kept behind **Show log** on its page.
- **Needs:** three things, and the install dialog asks for all three: your **zone** (`eu2.make.com` and the like) and **team id**, which are plain settings, and an **API key**, which is not — the key goes to the vault and reaches the process as `MAKE_API_KEY` at spawn time, never into the database (06 §5). Give the key only `scenarios:read` and `scenarios:run`.
- **Can reach:** the scenarios in that team. **A scenario can do anything you built it to do** — post to Slack, charge a card, mail a customer — so running one is an execute-tier call and the mode decides.
- **Protocol:** the package was checked against the npm registry when this entry was written (2026-09-12). Nobody here has spawned it. This is also the first entry in the catalogue that combines a `user_config` form with a vault-held key, which is the pair 03 §11 step 2 describes.
