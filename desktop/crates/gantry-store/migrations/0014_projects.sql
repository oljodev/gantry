-- 0014: projects (docs/plan/06 §3, 09 M11).
--
-- A project is a place to put related work and the context it shares: instructions, knowledge
-- files, a folder, and the defaults every chat started in it gets. `chats.project_id` has been
-- there since 0002 waiting for this table, as have `project_skills`, `memories.scope_id` and
-- `artifacts.project_id`.
--
-- Nothing here is executable and nothing is hidden: every row is something the user typed, a
-- file they chose, or a default they set, and deleting the project leaves its chats alone (they
-- come out of it rather than down with it — see the note on the foreign key below).

CREATE TABLE projects (
  id                       TEXT PRIMARY KEY,
  name                     TEXT NOT NULL,
  description              TEXT NOT NULL DEFAULT '',
  -- Prompt layer 5 (10 §2), capped at 8,000 characters by the layer that writes it.
  instructions             TEXT NOT NULL DEFAULT '',
  -- The folder every chat in the project starts with attached, and a code session's cwd.
  workspace_path           TEXT,
  -- Defaults for a new chat. NULL means "whatever the settings say", which is not the same as a
  -- value that happens to equal the settings: a project that does not care must not freeze
  -- today's setting into every chat it ever opens.
  default_mode             TEXT,              -- manual | auto_edit | plan | auto
  default_guard            INTEGER,           -- 0 | 1
  default_connectors_json  TEXT CHECK (default_connectors_json IS NULL OR json_valid(default_connectors_json)),
  default_grants_json      TEXT CHECK (default_grants_json IS NULL OR json_valid(default_grants_json)),
  pinned                   INTEGER NOT NULL DEFAULT 0,
  sort_order               INTEGER NOT NULL DEFAULT 0,
  created_at               INTEGER NOT NULL,
  updated_at               INTEGER NOT NULL,
  archived_at              INTEGER
);
CREATE INDEX projects_open ON projects (archived_at, pinned, sort_order, updated_at);

-- Knowledge: a file the user added to the project, kept as bytes in the blob store with the
-- text that goes into prompts beside it (06 §8). The same two-blob rule as an attachment — the
-- file is what they added, the text is what a prompt can hold — except that here the text is
-- the column, because it is read on every chat creation and a blob read per file is a read too
-- many for something that is in every prompt.
CREATE TABLE project_files (
  id             TEXT PRIMARY KEY,
  project_id     TEXT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
  name           TEXT NOT NULL,
  mime           TEXT NOT NULL,
  size           INTEGER NOT NULL,
  blob_hash      TEXT NOT NULL,
  extracted_text TEXT,
  created_at     INTEGER NOT NULL
);
CREATE INDEX project_files_by_project ON project_files (project_id, created_at);

-- 0012 left this table without its foreign key because `projects` did not exist; this is the
-- constraint joining it, which in SQLite means rebuilding the table. It is empty in every
-- database that exists — nothing could write it until now — so the copy is a formality that
-- keeps the migration honest rather than a data move.
CREATE TABLE project_skills_new (
  project_id TEXT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
  skill_id   TEXT NOT NULL REFERENCES skills (id) ON DELETE CASCADE,
  pinned_at  INTEGER NOT NULL,
  PRIMARY KEY (project_id, skill_id)
);
INSERT INTO project_skills_new (project_id, skill_id, pinned_at)
  SELECT project_id, skill_id, pinned_at FROM project_skills;
DROP TABLE project_skills;
ALTER TABLE project_skills_new RENAME TO project_skills;

-- `chats.project_id` stays without a foreign key, on purpose. A chat outlives its project:
-- deleting a project takes its instructions, its knowledge and its defaults, and leaves every
-- conversation held in it readable, with `project_id` set back to NULL by the delete. A cascade
-- here would delete the user's chats as a side effect of tidying up a folder.
CREATE INDEX chats_project ON chats (project_id, last_message_at);
