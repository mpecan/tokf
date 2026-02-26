-- no-transaction
-- CockroachDB rejects multiple DDL changes to the same table in a single
-- transaction ("schema change already in progress"). Run without a transaction
-- so each statement commits independently.

-- Make filter_hash nullable: stdlib/local filters have no registry entry.
ALTER TABLE usage_events ALTER COLUMN filter_hash DROP NOT NULL;

-- Drop the FK constraint so that local/stdlib filter hashes (not in the
-- registry) can be stored in usage_events without a referential-integrity error.
-- IF EXISTS guards against schema drift across CockroachDB versions.
ALTER TABLE usage_events DROP CONSTRAINT IF EXISTS usage_events_filter_hash_fkey;

-- Drop the FK constraint on filter_stats so rollup rows can be created for
-- hashes that are not (yet) in the filters registry.
ALTER TABLE filter_stats DROP CONSTRAINT IF EXISTS filter_stats_filter_hash_fkey;

-- Add human-readable label for display in /api/gain breakdowns.
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS filter_name TEXT;

-- NOTE: savings_pct was added to filter_stats by 20260226000000_add_savings_pct.sql (search feature).
-- No duplicate ALTER needed here.
