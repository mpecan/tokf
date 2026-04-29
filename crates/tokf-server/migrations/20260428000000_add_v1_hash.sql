-- Add `v1_hash` column for the schema-independent canonical TOML hash (ADR-0002).
--
-- Nullable on purpose: existing rows are NULL until the backfill endpoint
-- (`POST /api/filters/backfill-v1-hashes`) has populated them by re-reading
-- each row's TOML from R2. Adding NOT NULL or UNIQUE here would either fail
-- the migration or break the backfill mid-run as soon as the first
-- historical duplicate is encountered.
--
-- The dedup migration (separate PR) will collapse duplicate v1 rows and may
-- add UNIQUE alongside.
--
-- A non-partial index is used because CockroachDB cannot create a partial
-- index on a column added in the same migration ("column ... is not public",
-- error 0A000). The space cost of indexing NULL is negligible (NULLs are
-- already collapsed in B-tree indexes).
ALTER TABLE filters ADD COLUMN v1_hash TEXT;
CREATE INDEX filters_v1_hash_idx ON filters(v1_hash);
