-- 0011: when a model was released, as the provider's list reports it (docs/plan/02 §2).
-- The age filter in the model dialog needs a date, and a date is not a capability, so it is a
-- column rather than another field inside `capabilities_json`.
ALTER TABLE models ADD COLUMN created_at INTEGER;
