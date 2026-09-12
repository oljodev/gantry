# Exa

Exa's hosted search server: search the web by meaning, find pages like one you have, and get the
contents back rather than a list of links.

- **Runs:** nothing locally. `https://mcp.exa.ai/mcp`.
- **Needs:** an Exa API key, pasted at install. Exa reads it from the URL, so Gantry appends it as `?exaApiKey=…` rather than sending a header — the first connector in the catalogue where the key goes anywhere but `Authorization`.
- **Can reach:** the public web, through Exa. Your search terms go to Exa; nothing of yours is read.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): the server refuses the 2026-07-28 revision and is carried by the legacy handshake, which it completes and lists tools through. The recorded fixture is that list, and it is short: `web_search_exa` and `web_fetch_exa`, both reads.
