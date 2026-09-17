# Sub agents

**Runs** in Gantry itself. No process, no server, no network of its own.

**Needs** nothing: no key, no account, nothing to install. It is installed on first run and
cannot be removed, because the page that configures it is drawn whether or not it is there.

**Can reach** whatever the sub agent it starts is given, which is never more than the chat that
started it. A type may narrow that further: the built-in `researcher` gets the web connector and
read-only tools, and nothing else. A sub agent works in the same folders as the session that
started it and may only change files if its type says so.

**Protocol** — none. It is a native connector: one tool, `subagents__run`, implemented in
`gantry-agent` rather than in a crate of its own, because running a sub agent *is* running a turn
and the turn manager is what runs turns.

## What it does

`run` starts one sub agent and waits for it. The model names a type from the library, writes the
task, and gets back one report of text. Several calls in the same round run at once, up to the
limit in Settings → Sub agents.

A sub agent is a hidden chat row with a `parent_turn_id`: same runner, same event stream, same
guard, same transcript. Its permission cards are raised against the **parent's** chat and turn,
so they appear in the conversation the user is in, naming the agent that asked.

## Why it is not in the catalogue

`catalog.hidden` keeps it out of Discover, out of Your connectors and out of the model's own
connector search. It is app behaviour, like artifacts and memory — there is nothing to browse,
nothing to sign into and nothing to compare it with. It is still an ordinary instance
underneath, which is what the composer's checkbox attaches and what the Code surface turns on.

See `docs/plan/18-sub-agents.md`.
