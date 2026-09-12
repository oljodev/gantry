-- 0012: skills and memory (docs/plan/06 §3, 12).
--
-- Two features, one property: both only add text to a prompt. So there is nothing executable
-- in here, and every row is something the user can read on a page and delete.
--
-- The skills index is a *projection of files*, not the truth: a user skill lives in
-- `<app_data>/skills/<name>/SKILL.md` and can be edited with any editor or kept in Git, and a
-- bundled one lives in the binary. `content_hash` is what tells us the file moved under us.
-- `skill_versions` is the truth we keep of our own: every save, import, proposal and external
-- change snapshots the whole text, so Replace is always reversible.

CREATE TABLE skills (
  -- The name is the id, because it is also the folder name and what the user reads (12 §A2).
  id             TEXT PRIMARY KEY,
  source         TEXT NOT NULL,               -- bundled | user | imported
  path           TEXT,                        -- the folder; NULL for a bundled skill
  name           TEXT NOT NULL,
  description    TEXT NOT NULL,
  triggers_json  TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(triggers_json)),
  always_include INTEGER NOT NULL DEFAULT 0,
  enabled        INTEGER NOT NULL DEFAULT 1,
  content_hash   TEXT NOT NULL,
  size           INTEGER NOT NULL DEFAULT 0,
  version        INTEGER NOT NULL DEFAULT 1,
  author         TEXT,
  license        TEXT,
  references_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(references_json)),
  installed_at   INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL,
  last_used_at   INTEGER,
  use_count      INTEGER NOT NULL DEFAULT 0
);
-- Matching reads every enabled skill, which is a handful of rows; the index is for the list.
CREATE INDEX skills_enabled ON skills (enabled, name);

CREATE TABLE skill_versions (
  id         TEXT PRIMARY KEY,
  skill_id   TEXT NOT NULL REFERENCES skills (id) ON DELETE CASCADE,
  version    INTEGER NOT NULL,
  -- The whole `SKILL.md`, frontmatter included, so a restore needs nothing else.
  content    TEXT NOT NULL,
  source     TEXT NOT NULL,                   -- bundled | user_edit | import | ai_proposal | external_change
  created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX skill_versions_by_skill ON skill_versions (skill_id, version);

-- Pinning: a pinned skill is in the frozen prompt of every chat of that project or that chat,
-- and is never matched per message (12 §A4 rule 4).
CREATE TABLE chat_skills (
  chat_id   TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
  skill_id  TEXT NOT NULL REFERENCES skills (id) ON DELETE CASCADE,
  pinned_at INTEGER NOT NULL,
  PRIMARY KEY (chat_id, skill_id)
);
-- `projects` does not exist yet (it arrives with M11), so this one carries no foreign key to
-- it; the column is the project id all the same, and the constraint joins it when the table
-- lands rather than blocking this migration on a milestone.
CREATE TABLE project_skills (
  project_id TEXT NOT NULL,
  skill_id   TEXT NOT NULL REFERENCES skills (id) ON DELETE CASCADE,
  pinned_at  INTEGER NOT NULL,
  PRIMARY KEY (project_id, skill_id)
);

CREATE TABLE memories (
  id                TEXT PRIMARY KEY,
  scope_kind        TEXT NOT NULL,            -- global | project
  scope_id          TEXT,                     -- the project, when the scope is one
  kind              TEXT NOT NULL,            -- instruction | preference | fact | note
  text              TEXT NOT NULL,
  always_include    INTEGER NOT NULL DEFAULT 0,
  source            TEXT NOT NULL,            -- user | assistant
  origin_chat_id    TEXT,                     -- provenance: no foreign key, because deleting a
  origin_message_id TEXT,                     -- chat must not delete what it taught us
  tags_json         TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(tags_json)),
  enabled           INTEGER NOT NULL DEFAULT 1,
  use_count         INTEGER NOT NULL DEFAULT 0,
  last_used_at      INTEGER,
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL,
  -- Deleted, and restorable from Recently deleted for thirty days (12 §B5).
  archived_at       INTEGER
);
-- The core set is read at every chat creation: scope, then kind, then enabled.
CREATE INDEX memories_scope ON memories (scope_kind, scope_id, enabled, archived_at);

-- The long tail is found by full text (12 §B4). Its own copy of the text, for the reason the
-- other FTS tables have one: VACUUM may renumber the rowids of a table keyed by a string.
CREATE VIRTUAL TABLE memories_fts USING fts5 (
  memory_id UNINDEXED,
  text,
  tags,
  tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER memories_fts_insert AFTER INSERT ON memories BEGIN
  INSERT INTO memories_fts (memory_id, text, tags) VALUES (new.id, new.text, new.tags_json);
END;
CREATE TRIGGER memories_fts_delete AFTER DELETE ON memories BEGIN
  DELETE FROM memories_fts WHERE memory_id = old.id;
END;
CREATE TRIGGER memories_fts_update AFTER UPDATE OF text, tags_json ON memories BEGIN
  DELETE FROM memories_fts WHERE memory_id = old.id;
  INSERT INTO memories_fts (memory_id, text, tags) VALUES (new.id, new.text, new.tags_json);
END;

-- `chats.snapshot_memory_ids_json` — which memories a chat's frozen prompt was built from
-- (10 §6, 12 §B4) — is not added here: 0002 created the column with the rest of the chat row,
-- ahead of the feature that fills it. This migration is where it starts being written.
