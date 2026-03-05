-- Add stdlib flag to filters
ALTER TABLE filters ADD COLUMN is_stdlib BOOLEAN NOT NULL DEFAULT FALSE;

-- Service tokens for CI automation (separate from user bearer tokens)
CREATE TABLE service_tokens (
    id BIGSERIAL PRIMARY KEY,
    token_hash TEXT NOT NULL,
    description TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ
);
CREATE UNIQUE INDEX idx_service_tokens_hash ON service_tokens(token_hash);
