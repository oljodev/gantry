# LangSmith

LangSmith's hosted server: traces, runs, datasets and evaluation results.

- **Runs:** nothing locally. `https://api.smith.langchain.com/mcp`.
- **Needs:** a LangSmith account; sign-in happens in your browser and the server registers Gantry on the spot.
- **Can reach:** your traces and datasets — **which contain the prompts and answers your application handled**, and therefore whatever your users typed into it. Reading them in a chat is reading production data.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document, dynamic registration offered. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
