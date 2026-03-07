-- Partial index on deprecated_at for version queries.
-- Split from the column-add migration because CockroachDB cannot create
-- a partial index on a column added in the same transaction.

CREATE INDEX idx_filters_deprecated ON filters(deprecated_at) WHERE deprecated_at IS NOT NULL;
