-- P1.4: Add index on filter_stats(filter_hash) to speed up JOIN in search queries.
CREATE INDEX IF NOT EXISTS filter_stats_filter_hash_idx ON filter_stats (filter_hash);
