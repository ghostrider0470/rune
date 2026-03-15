CREATE TABLE paired_devices (
    id                UUID PRIMARY KEY,
    name              TEXT NOT NULL,
    public_key        TEXT NOT NULL UNIQUE,
    role              TEXT NOT NULL DEFAULT 'operator',
    scopes            JSONB NOT NULL DEFAULT '[]',
    token_hash        TEXT NOT NULL,
    token_expires_at  TIMESTAMPTZ NOT NULL,
    paired_at         TIMESTAMPTZ NOT NULL,
    last_seen_at      TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_paired_devices_token_hash ON paired_devices (token_hash);
CREATE INDEX idx_paired_devices_public_key ON paired_devices (public_key);

CREATE TABLE pairing_requests (
    id            UUID PRIMARY KEY,
    device_name   TEXT NOT NULL,
    public_key    TEXT NOT NULL,
    challenge     TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL,
    expires_at    TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_pairing_requests_expires ON pairing_requests (expires_at);
