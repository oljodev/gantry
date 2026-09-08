-- 0008: the two surfaces (docs/plan/16 §13). A code session is a chat with `surface = 'code'`
-- and at least one folder; everything downstream — messages, turns, events, tool calls, grants,
-- attached connectors — is surface-agnostic and stays that way.

ALTER TABLE chats ADD COLUMN surface TEXT NOT NULL DEFAULT 'chat'
  CHECK (surface IN ('chat', 'code'));

-- The two lists are the hot query, one per surface.
CREATE INDEX chats_by_surface ON chats (surface, archived_at, pinned, last_message_at);

-- The folders a session may reach (06 §3). A code session must have one before its first turn;
-- that is enforced in the agent, so the row can land in the same transaction as the message.
CREATE TABLE chat_roots (
  chat_id  TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
  path     TEXT NOT NULL,
  added_at INTEGER NOT NULL,
  PRIMARY KEY (chat_id, path)
);
