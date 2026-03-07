-- Add version tracking columns to filters table.
-- introduced_at: tokf semver when this filter first appeared in stdlib (NULL for community filters)
-- deprecated_at: tokf semver when this filter was removed from stdlib (NULL = still current)
-- successor_hash: content_hash of the replacement filter (NULL = no successor)

ALTER TABLE filters ADD COLUMN introduced_at TEXT;
ALTER TABLE filters ADD COLUMN deprecated_at TEXT;
ALTER TABLE filters ADD COLUMN successor_hash TEXT;

CREATE INDEX idx_filters_command_pattern ON filters(command_pattern);
