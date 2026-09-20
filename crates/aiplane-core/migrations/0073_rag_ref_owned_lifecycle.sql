-- A collection is configuration; a ref is the indexable and searchable unit.
-- Remove the obsolete collection lifecycle so no stale second status can be
-- surfaced after the ref-owned indexer completes a run.

ALTER TABLE rag_collections DROP COLUMN status;
ALTER TABLE rag_collections DROP COLUMN last_indexed_at;
ALTER TABLE rag_collections DROP COLUMN last_indexed_commit;
ALTER TABLE rag_collections DROP COLUMN last_error;
