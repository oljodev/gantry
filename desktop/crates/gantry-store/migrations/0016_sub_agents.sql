-- 0016: sub agents (docs/plan/18).
--
-- A sub agent is a chat with a parent. That is the whole storage decision: it buys the runner,
-- the event stream, the transcript, the usage accounting and the tool-call history for one
-- column, where an in-memory run would have bought none of them and nothing to read afterwards.
--
-- `parent_turn_id` is the turn that started it, not the chat, because "which of the parent's
-- turns spent this money" is the question anyone asks first, and the chat is one join away. The
-- cascade is deliberate: deleting a chat takes the transcripts of the sub agents its turns
-- started, which is what a person deleting a conversation means.
ALTER TABLE chats ADD COLUMN parent_turn_id TEXT REFERENCES turns(id) ON DELETE CASCADE;
ALTER TABLE chats ADD COLUMN agent_type TEXT;
CREATE INDEX chats_parent ON chats(parent_turn_id) WHERE parent_turn_id IS NOT NULL;

-- The library of agent types (18 §3). A type is a record rather than a file — unlike a skill,
-- which is a document a person writes and can keep in Git — because most of its fields are
-- switches and lists, and a document whose body is a form is a form.
--
-- `open_json` is the list of fields the *parent model* may set in the call. Everything not in it
-- is fixed by the type, and naming it in a call is refused rather than ignored.
CREATE TABLE agent_types (
  -- The slug the model names in a call, and what the user reads.
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  -- One line, read by the parent model when it chooses between types.
  description   TEXT NOT NULL,
  -- The system prompt fragment the sub agent runs under.
  instructions  TEXT NOT NULL DEFAULT '',
  -- inherit | rules | a "provider/model" string (18 §5).
  model         TEXT NOT NULL DEFAULT 'inherit',
  -- Namespaces it may use; the string "inherit" inside means the parent chat's own.
  connectors_json TEXT NOT NULL DEFAULT '["inherit"]' CHECK (json_valid(connectors_json)),
  -- NULL means the parent chat's mode and guard.
  mode          TEXT,
  guard         INTEGER,
  -- Whether it may change files in the session's folders, or only read them.
  write_files   INTEGER NOT NULL DEFAULT 0,
  memory        INTEGER NOT NULL DEFAULT 0,
  skills        INTEGER NOT NULL DEFAULT 0,
  open_json     TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(open_json)),
  -- Shipped with Gantry: editable, and Reset puts the original back.
  builtin       INTEGER NOT NULL DEFAULT 0,
  enabled       INTEGER NOT NULL DEFAULT 1,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

-- The two types that ship (18 §3, A4). One knows its job and fixes every field; one knows
-- nothing and lets the parent say everything, which is the shape a coding task wants.
INSERT INTO agent_types (id, name, description, instructions, model, connectors_json, mode, guard,
                         write_files, memory, skills, open_json, builtin, enabled, created_at, updated_at)
VALUES
  ('researcher', 'Researcher',
   'Reads the web and reports back. Give it a question and what you need out of it. It cannot change anything.',
   'You are a research sub agent. You were given one question by another model and your whole output is the answer to it.

Read before you answer. Search, then open the promising results with fetch_url and read them; a snippet is not a source. Use find_in_page on a long page rather than paging through it from the top. Quote what a page actually says, with the address you read it at, and say plainly when you could not find something rather than filling the gap from memory.

Answer the question you were given and nothing beside it. No preamble, no offer to continue, no questions back — nobody will read a question. Lead with the answer, then the evidence for it, then anything you could not settle.',
   'inherit', '["web"]', 'auto', 1, 0, 0, 0, '[]', 1, 1,
   unixepoch() * 1000, unixepoch() * 1000),
  ('agent', 'General agent',
   'A sub agent you brief yourself: write its instructions and say which tools it needs. Use it for work that is not research.',
   'You are a sub agent. Another model gave you the task below and will read your report; the user will not see this conversation and cannot answer you.

Do the work, then report: what you did, what you found, and anything the model that briefed you has to decide. No preamble and no questions back.',
   'inherit', '["inherit"]', NULL, NULL, 0, 0, 0,
   '["instructions","connectors","write","model"]', 1, 1,
   unixepoch() * 1000, unixepoch() * 1000);
