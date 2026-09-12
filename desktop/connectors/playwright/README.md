# Playwright

Microsoft's Playwright server as a local process: open a page, click through it, read what is
there, take a screenshot.

- **Runs:** `npx -y @playwright/mcp` on this machine, on Node 18 or later. Gantry checks for it before the install goes through and refuses with the missing runtime's name rather than failing later with an error about `npx` (03 §11 step 1), and whatever the server writes to stderr is kept behind **Show log** on its page.
- **Needs:** nothing to sign in to. The first run downloads a browser if Playwright has none, which takes a minute and some disk.
- **Can reach:** your machine and your network: it opens whatever URL it is given, **as you** — your VPN, your localhost, your intranet. A page can be a source of instructions the model reads, which is the `suspicious_input` case of the guard, so this is a connector worth running in a chat you are watching.
- **Protocol:** the package was checked against the npm registry when this entry was written (2026-09-12) and `@playwright/mcp` is published and current. Nobody here has spawned it: `--spawn` in the probe is still the gap 17 §5 names, so the tool list is whatever the first connection discovers.
