CREATE TABLE device_flows (
    id           BIGSERIAL PRIMARY KEY,
    device_code  TEXT NOT NULL UNIQUE,
    user_code    TEXT NOT NULL,
    verification_uri TEXT NOT NULL,
    interval_secs INT NOT NULL DEFAULT 5,
    ip_address   TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending'
                 CHECK (status IN ('pending', 'completed', 'expired', 'denied')),
    user_id      BIGINT REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at   TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ
);

CREATE INDEX ON device_flows(ip_address, created_at);
CREATE INDEX ON device_flows(expires_at);
