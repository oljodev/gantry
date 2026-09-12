# Azure

Microsoft's Azure server as a local process: subscriptions, resources, configuration and logs.

- **Runs:** `npx -y @azure/mcp server start` on this machine, on Node 20 or later. Gantry checks for it before the install goes through and refuses with the missing runtime's name rather than failing later with an error about `npx` (03 §11 step 1), and whatever the server writes to stderr is kept behind **Show log** on its page.
- **Needs:** the Azure CLI, signed in. **There is no credential to paste here**: the server uses the same token `az login` left on this machine, which means this connector is as powerful as your own CLI session and no less.
- **Can reach:** every subscription that session can reach. That is a wide door, and the reason to attach this to a chat deliberately rather than leave it on.
- **Protocol:** the package was checked against the npm registry when this entry was written (2026-09-12); `@azure/mcp` is published, currently as a beta, which is Microsoft's own label for it. Nobody here has spawned it.
