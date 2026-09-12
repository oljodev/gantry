-- Incognito chats (docs/plan/15 A21): a conversation that never joins the history.
--
-- The row exists while the window is open, because everything a turn does — messages, turns,
-- tool calls, artifacts — hangs off a chat id, and a second, parallel, in-memory path for all
-- of it would be a large amount of code whose only property is that it is untested. What makes
-- the promise true instead is that the row is never listed, never searched, and deleted when
-- the window closes; startup deletes any that a crash left behind, so the worst case is that an
-- incognito chat outlives its window and not that it joins the sidebar.
ALTER TABLE chats ADD COLUMN incognito INTEGER NOT NULL DEFAULT 0;
