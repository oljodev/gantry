-- 0005: standing permissions for one chat (docs/plan/04 §8, 06 §3). A grant only ever turns a
-- prompt into an allow; it is never consulted to lift a denial.

CREATE TABLE chat_grants (
  id             TEXT PRIMARY KEY,
  chat_id        TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
  instance_id    TEXT NOT NULL,                   -- connector id, or `gantry` for runtime tools
  instance_name  TEXT NOT NULL,
  tool_name      TEXT,                            -- NULL = every tool of the instance
  tier_ceiling   TEXT,                            -- NULL, or read | write | write_external | execute | destructive
  arg_scope_json TEXT CHECK (arg_scope_json IS NULL OR json_valid(arg_scope_json)),
  source         TEXT NOT NULL,                   -- user_prompt | access_request | project_default
  created_at     INTEGER NOT NULL,
  revoked_at     INTEGER
);
CREATE INDEX chat_grants_chat ON chat_grants (chat_id, revoked_at);
