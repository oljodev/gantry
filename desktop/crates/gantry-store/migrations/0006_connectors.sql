-- 0006: installed connectors (docs/plan/03 §3, §7, 06 §3). Configuration only: a command, a
-- URL, the *names* of environment variables and headers. Every secret value is a row in
-- `credentials`, encrypted, and is referenced from here by id at most.

CREATE TABLE connector_instances (
  id                TEXT PRIMARY KEY,
  catalog_id        TEXT,                          -- NULL for a server the user added by hand
  namespace         TEXT NOT NULL UNIQUE,          -- the tool prefix: `github`, `custom-1`
  display_name      TEXT NOT NULL,
  kind              TEXT NOT NULL,                 -- native | mcp-stdio | mcp-remote
  config_json       TEXT NOT NULL CHECK (json_valid(config_json)),
  user_config_json  TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(user_config_json)),
  auth_type         TEXT NOT NULL,                 -- none | api_key | headers | oauth2
  auth_state        TEXT NOT NULL,
  credential_id     TEXT,                          -- the token or key in `credentials`
  enabled           INTEGER NOT NULL DEFAULT 1,
  tools_cache_json  TEXT CHECK (tools_cache_json IS NULL OR json_valid(tools_cache_json)),
  server_info_json  TEXT CHECK (server_info_json IS NULL OR json_valid(server_info_json)),
  installed_at      INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL,
  last_connected_at INTEGER,
  last_error        TEXT
);
CREATE INDEX connector_instances_catalog ON connector_instances (catalog_id);

-- One registered OAuth client per issuer per instance (03 §7 step 2). The client secret, when
-- an issuer insists on one, is a credential; only its id would be stored beside this row.
CREATE TABLE oauth_clients (
  id                TEXT PRIMARY KEY,
  instance_id       TEXT NOT NULL REFERENCES connector_instances (id) ON DELETE CASCADE,
  issuer            TEXT NOT NULL,
  client_id         TEXT NOT NULL,
  registration_json TEXT CHECK (registration_json IS NULL OR json_valid(registration_json)),
  created_at        INTEGER NOT NULL
);
CREATE UNIQUE INDEX oauth_clients_instance_issuer ON oauth_clients (instance_id, issuer);

-- Which connectors a chat may use (06 §3). A connector is installed once and attached per chat,
-- so installing something never changes what an existing conversation can reach.
CREATE TABLE chat_connectors (
  chat_id          TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
  instance_id      TEXT NOT NULL REFERENCES connector_instances (id) ON DELETE CASCADE,
  tool_filter_json TEXT CHECK (tool_filter_json IS NULL OR json_valid(tool_filter_json)),
  source           TEXT NOT NULL,                  -- user | project_default | access_request | suggestion
  attached_at      INTEGER NOT NULL,
  PRIMARY KEY (chat_id, instance_id)
);
