# Salesforce

Salesforce's own server as a local process: orgs, metadata, SOQL and deploys.

- **Runs:** `npx -y @salesforce/mcp --orgs DEFAULT_TARGET_ORG --toolsets all` on this machine, on Node 20 or later. Gantry checks for it before the install goes through and refuses with the missing runtime's name rather than failing later with an error about `npx` (03 §11 step 1), and whatever the server writes to stderr is kept behind **Show log** on its page.
- **Needs:** the Salesforce CLI with an authorised org. **There is no credential to paste**; the arguments here say which orgs the server may use, and the default is the CLI's default target org rather than all of them.
- **Can reach:** that org. A deploy or an update changes a live Salesforce org, which is somebody's working day, so those calls follow the chat's mode and a destructive one is confirmed every time.
- **Protocol:** the package was checked against the npm registry when this entry was written (2026-09-12). Nobody here has spawned it.
