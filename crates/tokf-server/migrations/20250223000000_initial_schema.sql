CREATE TABLE users (
    id BIGSERIAL PRIMARY KEY,
    github_id BIGINT NOT NULL,
    username TEXT NOT NULL,
    avatar_url TEXT NOT NULL,
    profile_url TEXT NOT NULL,
    visible BOOLEAN NOT NULL DEFAULT TRUE,
    orgs JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT users_github_id_unique UNIQUE (github_id)
);

CREATE TABLE auth_tokens (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,   -- NULL means the token never expires
    last_used_at TIMESTAMPTZ
);

CREATE TABLE machines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    hostname TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_sync_at TIMESTAMPTZ
);

CREATE TABLE filters (
    content_hash TEXT PRIMARY KEY,
    command_pattern TEXT NOT NULL,
    canonical_command TEXT NOT NULL,
    author_id BIGINT NOT NULL REFERENCES users(id),
    r2_key TEXT NOT NULL,
    test_r2_key TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE filter_tests (
    id BIGSERIAL PRIMARY KEY,
    filter_hash TEXT NOT NULL REFERENCES filters(content_hash) ON DELETE CASCADE,
    r2_key TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE usage_events (
    id BIGSERIAL PRIMARY KEY,
    filter_hash TEXT NOT NULL REFERENCES filters(content_hash) ON DELETE CASCADE,
    machine_id UUID NOT NULL REFERENCES machines(id),
    input_tokens BIGINT NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    output_tokens BIGINT NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    command_count INT NOT NULL DEFAULT 0 CHECK (command_count >= 0),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE sync_cursors (
    machine_id UUID PRIMARY KEY REFERENCES machines(id) ON DELETE CASCADE,
    last_event_id BIGINT NOT NULL DEFAULT 0,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE filter_stats (
    filter_hash TEXT PRIMARY KEY REFERENCES filters(content_hash) ON DELETE CASCADE,
    total_commands BIGINT NOT NULL DEFAULT 0,
    total_input_tokens BIGINT NOT NULL DEFAULT 0,
    total_output_tokens BIGINT NOT NULL DEFAULT 0,
    last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes per spec
CREATE INDEX ON users(github_id);
CREATE INDEX ON auth_tokens(user_id);
CREATE UNIQUE INDEX ON auth_tokens(token_hash);
CREATE INDEX ON auth_tokens(expires_at);
CREATE INDEX ON machines(user_id);
CREATE INDEX ON machines(last_sync_at);
CREATE INDEX ON filters(author_id);
CREATE INDEX ON filter_tests(filter_hash);
CREATE INDEX ON usage_events(machine_id, recorded_at);
CREATE INDEX ON usage_events(filter_hash);
