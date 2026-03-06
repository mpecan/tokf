ALTER TABLE usage_events ADD COLUMN raw_tokens BIGINT NOT NULL DEFAULT 0 CHECK (raw_tokens >= 0);
ALTER TABLE filter_stats ADD COLUMN total_raw_tokens BIGINT NOT NULL DEFAULT 0;
