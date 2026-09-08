-- 0007: the tool schemas a connector discovered (docs/plan/03 §6).
--
-- `tools_cache_json` holds what the UI lists: names, descriptions, tiers. The model needs more
-- than that — the JSON Schema of each tool's arguments — and rebuilding the registry from the
-- UI cache after a restart declared every tool as a bare object, so the model had to guess
-- parameter names until someone pressed Refresh. This column keeps the full definitions.

ALTER TABLE connector_instances ADD COLUMN tool_defs_json TEXT
  CHECK (tool_defs_json IS NULL OR json_valid(tool_defs_json));
