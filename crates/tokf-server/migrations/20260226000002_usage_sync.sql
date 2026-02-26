-- Make filter_hash nullable: stdlib/local filters have no registry entry.
ALTER TABLE usage_events ALTER COLUMN filter_hash DROP NOT NULL;

-- Drop the FK constraint so that local/stdlib filter hashes (not in the
-- registry) can be stored in usage_events without a referential-integrity error.
ALTER TABLE usage_events DROP CONSTRAINT usage_events_filter_hash_fkey;

-- Drop the FK constraint on filter_stats so rollup rows can be created for
-- hashes that are not (yet) in the filters registry.
ALTER TABLE filter_stats DROP CONSTRAINT filter_stats_filter_hash_fkey;

-- Add human-readable label for display in /api/gain breakdowns.
ALTER TABLE usage_events ADD COLUMN filter_name TEXT;

-- Add savings_pct to filter_stats (missing from initial schema; needed by search ranking + /api/gain/filter/:hash).
ALTER TABLE filter_stats ADD COLUMN savings_pct FLOAT8 NOT NULL DEFAULT 0.0;
