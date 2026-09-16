-- The blob refcount goes (06 §3). It was a second copy of a fact the database already held —
-- a blob is referenced when a row references it — and every path that forgot to keep the copy
-- in step was a bug nothing could see. Four of them had: project knowledge files were never
-- counted at all, a document's extracted text was never counted, the edit journal counted up
-- and never down, and nothing counted generated media. The sweep now asks the referencing
-- tables instead, so a reference exists exactly when a row says it does.
ALTER TABLE blobs DROP COLUMN refcount;
