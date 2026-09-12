# Netlify

Netlify's hosted server, connected to your team: list sites, read build and deploy logs, manage
environment variables, and deploy.

- **Runs:** nothing locally. `https://mcp.netlify.com/mcp`.
- **Needs:** a Netlify account, signed in through your browser. Dynamic registration.
- **Can reach:** your Netlify team — its sites, their configuration and their logs. Environment
  variables are secrets, and a tool that reads them hands them to the model.
- **Deploying is confirmed every time.** `deploy-site` is overridden to `destructive` with
  `always_confirm`, not because it destroys anything but because it changes what the public sees
  and no mode should be able to wave that through (03 §6, 17 §6's last row).
- **Protocol:** verified mechanically when this entry was written (2026-09-11): `401` with a
  protected-resource document and dynamic registration offered. Nobody here has signed in, so the
  tool list is whatever the first connection discovers — if the deploy tool is named something
  else there, the override needs its name and this README needs correcting.
