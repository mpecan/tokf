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

-- `updated_at` drives the backfill cursor (`backfill_v1_hashes`). Ordering by
-- it instead of `created_at` keeps the backfill fair: every attempt — success
-- or failure — bumps `updated_at` to NOW(), so a permanently-failing row
-- (missing R2 object, un-canonicalisable TOML) moves to the back of the queue
-- rather than parking at the front of a `created_at ASC` scan and starving
-- newer rows.
--
-- Existing rows take the NOW() default at migration time (all roughly equal,
-- with `content_hash` as the cursor's deterministic tie-break). We deliberately
-- do NOT `UPDATE ... SET updated_at = created_at` here: CockroachDB rejects
-- referencing a column added earlier in the same migration ("column ... does
-- not exist", error 42703), the same not-yet-public constraint noted above.
-- Initial-pass ordering is cosmetic — fairness comes from the per-attempt bump,
-- not the starting order — so the alignment isn't worth a second migration.
ALTER TABLE filters ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
