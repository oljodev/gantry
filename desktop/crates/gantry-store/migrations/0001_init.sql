-- 0001: settings, providers, models and credentials (docs/plan/06 §3). Chats arrive in 0002.
-- The schema version is SQLite's `user_version` pragma, managed by rusqlite_migration.

CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE credentials (
  id         TEXT PRIMARY KEY,
  kind       TEXT NOT NULL,          -- api_key | oauth_token | oauth_client_secret | user_config_secret | search_api_key
  owner_kind TEXT NOT NULL,          -- provider | instance
  owner_id   TEXT NOT NULL,
  label      TEXT,
  ciphertext BLOB NOT NULL,
  nonce      BLOB NOT NULL,
  expires_at INTEGER,
  meta_json  TEXT NOT NULL DEFAULT '{}',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX credentials_owner ON credentials (owner_kind, owner_id);

CREATE TABLE providers (
  id            TEXT PRIMARY KEY,    -- anthropic | openai | google | xai | openrouter | custom:<ulid>
  kind          TEXT NOT NULL,       -- anthropic | openai_responses | openai_chat | gemini
  label         TEXT NOT NULL,
  base_url      TEXT,
  enabled       INTEGER NOT NULL DEFAULT 1,
  credential_id TEXT REFERENCES credentials (id) ON DELETE SET NULL,
  default_model TEXT,
  options_json  TEXT NOT NULL DEFAULT '{}',
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

CREATE TABLE models (
  provider_id       TEXT NOT NULL REFERENCES providers (id) ON DELETE CASCADE,
  model_id          TEXT NOT NULL,
  display_name      TEXT NOT NULL,
  capabilities_json TEXT NOT NULL,
  context_window    INTEGER,
  max_output        INTEGER,
  pricing_json      TEXT,
  fetched_at        INTEGER NOT NULL,
  PRIMARY KEY (provider_id, model_id)
);
