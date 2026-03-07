-- Track which tokf release version published each stdlib filter.
-- NULL for community filters and stdlib filters published before this migration.
ALTER TABLE filters ADD COLUMN IF NOT EXISTS stdlib_version TEXT;
