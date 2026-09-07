-- 0004: artifacts (docs/plan/13 §7, 06 §3). Every change is a new immutable version whose
-- content lives in the blob store; `artifact_kv` (13 §8) is deferred and not created.

CREATE TABLE artifacts (
  id                     TEXT PRIMARY KEY,
  chat_id                TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
  project_id             TEXT,                              -- denormalized from the chat
  type                   TEXT NOT NULL,                     -- markdown | code | html | svg | mermaid | react
  title                  TEXT NOT NULL,
  language               TEXT,
  summary                TEXT,
  current_version        INTEGER NOT NULL DEFAULT 1,
  created_by_message_id  TEXT,
  created_at             INTEGER NOT NULL,
  updated_at             INTEGER NOT NULL,
  archived_at            INTEGER
);
CREATE INDEX artifacts_chat ON artifacts (chat_id, updated_at);
CREATE INDEX artifacts_project ON artifacts (project_id, updated_at);

CREATE TABLE artifact_versions (
  id                 TEXT PRIMARY KEY,
  artifact_id        TEXT NOT NULL REFERENCES artifacts (id) ON DELETE CASCADE,
  version            INTEGER NOT NULL,
  content_blob_hash  TEXT NOT NULL,
  data_blob_hash     TEXT,                                  -- reserved for data types (13 §3)
  source             TEXT NOT NULL,                         -- model_create | model_update | model_edit | user_edit | user_restore
  tool_call_id       TEXT,
  message_id         TEXT,
  note               TEXT,
  size               INTEGER NOT NULL,
  created_at         INTEGER NOT NULL,
  UNIQUE (artifact_id, version)
);

-- Title, summary and the current version's text, replaced by the repository on every version.
CREATE VIRTUAL TABLE artifacts_fts USING fts5 (
  artifact_id UNINDEXED,
  chat_id UNINDEXED,
  title,
  summary,
  text,
  tokenize = 'unicode61'
);
