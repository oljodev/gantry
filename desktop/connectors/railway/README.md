# Railway

Railway's hosted server: projects and services, deployment and build logs, variables, and a
redeploy when you ask for one.

- **Runs:** nothing locally. `https://mcp.railway.com/mcp`.
- **Needs:** a Railway account. Sign-in happens in your browser; the authorization server is `backboard.railway.com` and it registers Gantry as a client on the spot.
- **Can reach:** the workspaces your account belongs to. Reading logs is the common case; a deploy or a variable change touches something your users are using, and is confirmed every time.
- **Protocol:** verified mechanically when this entry was written (2026-09-12), and it is the odd one in this batch: the server hands out its **whole tool list without a credential** and refuses every call until you sign in. The recorded fixture is therefore a real tool list beside a `401`-shaped auth document, and `fixtures/tools.json` here is the one in the catalogue that shows both.

Railway is the one connector in the catalogue whose tool list was recorded **without** a
credential, so its tiers are not guesswork. Fourteen tools were reviewed against 17 §6 and
carry an override: the five `delete-*` are `destructive` and confirmed every time, and
`accept-deploy`, `create-deployment`, `redeploy`, `restart-service`, `set-variables`,
`set-feature-flag`, `create-tcp-proxy`, `generate-domain` and `railway-agent` are confirmed
whatever the mode, because each one changes what is running or what the public internet can
reach. `list-variables` is confirmed too, and it is the interesting one: it only reads, but
what it reads is every environment variable of a service **fully rendered** — which is to say
every key and password in it. That is the "reads a credential" rule of the guard, applied in
the manifest so it holds before any guard is asked.

## Six new tools, found by the probe (2026-09-16)

Re-probing on the way past added `search-templates`, `describe-template`, `deploy-template`,
`create-function`, `get-function-source-code` and `update-function-source-code` to the recorded
fixture — the first drift this catalogue has caught on a live server, which is what the weekly
probe of 17 §5 exists for. Three of them earned an override on the same reasoning as the
fourteen above: `create-function` deploys a service the moment it is called, `deploy-template`
creates and deploys every service a template declares, and `update-function-source-code`
overwrites a running function's whole file — it does not patch it, so anything left out is gone.
The three reads (`search-templates`, `describe-template`, `get-function-source-code`) keep the
entry's default tier.
