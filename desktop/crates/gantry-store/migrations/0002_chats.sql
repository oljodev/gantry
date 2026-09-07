-- 0002: chats, turns, messages, attachments, events, blobs and full-text search
-- (docs/plan/06 §3). Chats leave memory with M2.

CREATE TABLE chats (
  id                       TEXT PRIMARY KEY,
  project_id               TEXT,
  title                    TEXT NOT NULL,
  title_source             TEXT NOT NULL DEFAULT 'auto',   -- auto | user
  pinned                   INTEGER NOT NULL DEFAULT 0,
  permission_mode          TEXT NOT NULL,                  -- manual | auto_edit | plan | auto
  auto_guard               INTEGER NOT NULL DEFAULT 1,
  provider_id              TEXT NOT NULL,
  model_id                 TEXT NOT NULL,
  effort                   TEXT NOT NULL,                  -- off | low | medium | high | max
  web_search               INTEGER NOT NULL DEFAULT 0,
  instructions             TEXT NOT NULL DEFAULT '',       -- chat-level custom instructions (10 §2)
  system_snapshot          TEXT NOT NULL,                  -- the frozen system prompt (02 §6)
  system_snapshot_version  INTEGER NOT NULL,
  snapshot_memory_ids_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(snapshot_memory_ids_json)),
  declared_tools_json      TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(declared_tools_json)),
  created_at               INTEGER NOT NULL,
  updated_at               INTEGER NOT NULL,
  last_message_at          INTEGER NOT NULL,
  archived_at              INTEGER
);
CREATE INDEX chats_sidebar ON chats (archived_at, pinned, last_message_at);

CREATE TABLE turns (
  id               TEXT PRIMARY KEY,
  chat_id          TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
  seq              INTEGER NOT NULL,
  status           TEXT NOT NULL,                          -- running | completed | cancelled | failed | interrupted
  provider_id      TEXT NOT NULL,
  model_id         TEXT NOT NULL,
  started_at       INTEGER NOT NULL,
  ended_at         INTEGER,
  usage_json       TEXT CHECK (usage_json IS NULL OR json_valid(usage_json)),
  stop_reason_json TEXT CHECK (stop_reason_json IS NULL OR json_valid(stop_reason_json)),
  error_json       TEXT CHECK (error_json IS NULL OR json_valid(error_json)),
  feedback         TEXT,                                   -- good | bad
  tool_call_count  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX turns_chat ON turns (chat_id, seq);
CREATE INDEX turns_status ON turns (status);

-- The transcript: append-only, ordered by seq inside a chat. `text` is the concatenated text
-- parts, kept for search; `parts_json` is the truth.
CREATE TABLE messages (
  id              TEXT PRIMARY KEY,
  chat_id         TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
  turn_id         TEXT REFERENCES turns (id) ON DELETE CASCADE,
  seq             INTEGER NOT NULL,
  role            TEXT NOT NULL,                           -- user | assistant | tool | system
  parts_json      TEXT NOT NULL CHECK (json_valid(parts_json)),
  text            TEXT NOT NULL DEFAULT '',
  origin_provider TEXT,
  stop_reason     TEXT,
  usage_json      TEXT CHECK (usage_json IS NULL OR json_valid(usage_json)),
  created_at      INTEGER NOT NULL
);
CREATE INDEX messages_chat ON messages (chat_id, seq);
CREATE INDEX messages_turn ON messages (turn_id);

CREATE TABLE blobs (
  hash       TEXT PRIMARY KEY,                             -- SHA-256, lowercase hex
  size       INTEGER NOT NULL,
  mime       TEXT,
  refcount   INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

CREATE TABLE attachments (
  id             TEXT PRIMARY KEY,
  message_id     TEXT NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
  chat_id        TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
  name           TEXT NOT NULL,
  mime           TEXT NOT NULL,
  size           INTEGER NOT NULL,
  blob_hash      TEXT NOT NULL REFERENCES blobs (hash),
  extracted_text TEXT,
  created_at     INTEGER NOT NULL
);
CREATE INDEX attachments_message ON attachments (message_id);

-- The append-only activity log: the persisted subset of agent events (05 §2).
CREATE TABLE events (
  id           TEXT PRIMARY KEY,
  chat_id      TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
  turn_id      TEXT NOT NULL REFERENCES turns (id) ON DELETE CASCADE,
  seq          INTEGER NOT NULL,
  ts           INTEGER NOT NULL,
  kind         TEXT NOT NULL,
  ref_id       TEXT,
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
);
CREATE INDEX events_turn ON events (chat_id, turn_id, seq);

-- Search. The FTS tables hold their own copy of the text rather than pointing at the content
-- tables by rowid, because VACUUM (Settings → Advanced) may renumber rowids of tables whose
-- primary key is not an integer. Triggers keep them in step.
CREATE VIRTUAL TABLE messages_fts USING fts5 (
  message_id UNINDEXED,
  chat_id UNINDEXED,
  text,
  tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts (message_id, chat_id, text) VALUES (new.id, new.chat_id, new.text);
END;
CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
  DELETE FROM messages_fts WHERE message_id = old.id;
END;
CREATE TRIGGER messages_fts_update AFTER UPDATE OF text ON messages BEGIN
  DELETE FROM messages_fts WHERE message_id = old.id;
  INSERT INTO messages_fts (message_id, chat_id, text) VALUES (new.id, new.chat_id, new.text);
END;

CREATE VIRTUAL TABLE chats_fts USING fts5 (
  chat_id UNINDEXED,
  title,
  tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER chats_fts_insert AFTER INSERT ON chats BEGIN
  INSERT INTO chats_fts (chat_id, title) VALUES (new.id, new.title);
END;
CREATE TRIGGER chats_fts_delete AFTER DELETE ON chats BEGIN
  DELETE FROM chats_fts WHERE chat_id = old.id;
END;
CREATE TRIGGER chats_fts_update AFTER UPDATE OF title ON chats BEGIN
  DELETE FROM chats_fts WHERE chat_id = old.id;
  INSERT INTO chats_fts (chat_id, title) VALUES (new.id, new.title);
END;
