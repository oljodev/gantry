-- 0003: tool calls and interactions (docs/plan/06 §3). Both are projections of the events
-- log, kept in step by the persister; `events` stays the truth.

CREATE TABLE tool_calls (
  id               TEXT PRIMARY KEY,                       -- the call id the model used
  chat_id          TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
  turn_id          TEXT NOT NULL REFERENCES turns (id) ON DELETE CASCADE,
  message_id       TEXT NOT NULL,
  instance_id      TEXT NOT NULL,                          -- connector id, or `gantry` for runtime tools
  connector_name   TEXT NOT NULL,
  tool_name        TEXT NOT NULL,
  model_tool_name  TEXT NOT NULL,
  args_json        TEXT NOT NULL CHECK (json_valid(args_json)),
  tier             TEXT NOT NULL,                          -- read | write | write_external | execute | destructive | app
  status           TEXT NOT NULL,                          -- proposed | awaiting_decision | denied | running | completed | failed | cancelled
  decision_source  TEXT,                                   -- mode | grant | user_once | user_chat_grant | judge | guardrail | scope | plan_mode
  judge_json       TEXT CHECK (judge_json IS NULL OR json_valid(judge_json)),
  display_json     TEXT NOT NULL CHECK (json_valid(display_json)),
  result_preview   TEXT,
  result_blob_hash TEXT,
  is_error         INTEGER NOT NULL DEFAULT 0,
  created_at       INTEGER NOT NULL,
  started_at       INTEGER,
  ended_at         INTEGER,
  duration_ms      INTEGER
);
CREATE INDEX tool_calls_turn ON tool_calls (chat_id, turn_id, created_at);
CREATE INDEX tool_calls_status ON tool_calls (status);

CREATE TABLE interactions (
  id              TEXT PRIMARY KEY,
  chat_id         TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
  turn_id         TEXT NOT NULL REFERENCES turns (id) ON DELETE CASCADE,
  kind            TEXT NOT NULL,                           -- permission | access_request | …
  payload_json    TEXT NOT NULL CHECK (json_valid(payload_json)),
  status          TEXT NOT NULL,                           -- pending | resolved | cancelled | expired
  resolution_json TEXT CHECK (resolution_json IS NULL OR json_valid(resolution_json)),
  created_at      INTEGER NOT NULL,
  resolved_at     INTEGER
);
CREATE INDEX interactions_pending ON interactions (chat_id, status);
