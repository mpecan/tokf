-- Schema-independent canonical TOML hash (ADR-0002).
--
-- Nullable on purpose: existing rows are NULL until the backfill endpoint
-- (`POST /api/filters/backfill-v1-hashes`) has populated them by re-reading
-- each row's TOML from R2. NOT NULL or UNIQUE here would either fail the
-- migration or break the backfill mid-run on the first historical duplicate;
-- both belong to the dedup migration (separate PR) which also collapses
-- duplicate v1 rows.
--
-- Non-partial index because CockroachDB rejects a partial index on a column
-- added in the same migration ("column ... is not public", error 0A000).
-- B-tree NULL handling makes the space cost negligible.
ALTER TABLE filters ADD COLUMN v1_hash TEXT;
CREATE INDEX filters_v1_hash_idx ON filters(v1_hash);
