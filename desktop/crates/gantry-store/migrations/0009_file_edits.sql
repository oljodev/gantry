-- 0009: the edit journal (docs/plan/06 §3, docs/connectors/code-editor.md §4). One row per
-- change a connector makes to a file: what it was, what it became, and the hunks between them.
-- It is what the Changes pane lists, what `undo` replays, and what Revert replays from the
-- other side. `events` stays the truth; this is the indexed projection of it.

CREATE TABLE file_edits (
  id                  TEXT PRIMARY KEY,
  -- The call that made the change. Not a foreign key: the journal is written while the call is
  -- still running, and an edit that outlives its projection row is still true.
  tool_call_id        TEXT NOT NULL,
  chat_id             TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
  path                TEXT NOT NULL,
  op                  TEXT NOT NULL,             -- create | modify | delete | rename
  before_blob_hash    TEXT,                      -- absent when the file did not exist
  after_blob_hash     TEXT,                      -- absent when the file was deleted
  hunks_json          TEXT NOT NULL CHECK (json_valid(hunks_json)),
  stats_json          TEXT NOT NULL CHECK (json_valid(stats_json)),
  applied_at          INTEGER NOT NULL,
  reverted_at         INTEGER,
  reverted_by_edit_id TEXT
);

-- "Edits to this file", newest first, which is what undo and the per-file Revert both ask.
CREATE INDEX file_edits_by_path ON file_edits (chat_id, path, applied_at);
CREATE INDEX file_edits_by_call ON file_edits (tool_call_id);
