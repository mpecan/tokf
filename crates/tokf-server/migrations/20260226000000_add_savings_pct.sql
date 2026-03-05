ALTER TABLE filter_stats ADD COLUMN savings_pct FLOAT8 NOT NULL DEFAULT 0.0;
CREATE INDEX ON filters(command_pattern);
