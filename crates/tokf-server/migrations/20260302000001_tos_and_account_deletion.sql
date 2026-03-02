-- no-transaction
-- CockroachDB rejects multiple DDL changes to the same table in a single
-- transaction. Run without a transaction so each statement commits independently.

-- tos_acceptances: append-only audit log of Terms of Service acceptances.
CREATE TABLE IF NOT EXISTS tos_acceptances (
    id          BIGSERIAL PRIMARY KEY,
    user_id     BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tos_version INT NOT NULL,
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ip_address  TEXT
);

CREATE INDEX IF NOT EXISTS idx_tos_acceptances_user_version
    ON tos_acceptances(user_id, tos_version);

-- Add a deleted_at column to users for soft-delete (account deletion).
-- When a user deletes their account, we anonymize the row and set deleted_at.
-- This preserves filter author_id references (filters are community resources).
ALTER TABLE users ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ;
