-- 0010: where a moved or copied file came from (docs/plan/06 §3).
--
-- `file_edits` was written for edits in place, where one path is the whole story. A move and a
-- copy have two ends, and Revert cannot undo either without knowing the other; the activity row
-- shows both ends on one line for the same reason (docs/connectors/filesystem.md §10).

ALTER TABLE file_edits ADD COLUMN from_path TEXT;
