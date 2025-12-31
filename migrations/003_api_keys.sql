-- API keys table for multi-user authentication
CREATE TABLE IF NOT EXISTS api_keys (
    id              VARCHAR(36) PRIMARY KEY,
    user_id         VARCHAR(255) NOT NULL,
    user_email      VARCHAR(255) NOT NULL,
    key_prefix      VARCHAR(30) NOT NULL UNIQUE,
    key_hash        VARCHAR(64) NOT NULL,
    name            VARCHAR(255) NOT NULL,
    scopes          TEXT[] NOT NULL,
    rate_limit_tier VARCHAR(20) NOT NULL DEFAULT 'standard',
    last_used_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at      TIMESTAMPTZ,

    CONSTRAINT key_prefix_format
        CHECK (key_prefix ~ '^sk_(live|test)_[a-zA-Z0-9_-]+$')
);

CREATE INDEX idx_api_keys_prefix ON api_keys(key_prefix) WHERE revoked_at IS NULL;
CREATE INDEX idx_api_keys_user ON api_keys(user_id);
