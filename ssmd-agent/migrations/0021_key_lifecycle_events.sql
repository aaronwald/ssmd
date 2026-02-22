-- migrate:up

-- Add disabled_at for temporary key suspension (revoked_at is permanent)
ALTER TABLE api_keys ADD COLUMN disabled_at TIMESTAMPTZ;

-- Audit trail for all key mutations
CREATE TABLE api_key_events (
    id          BIGSERIAL PRIMARY KEY,
    key_prefix  VARCHAR(30) NOT NULL,
    event_type  VARCHAR(32) NOT NULL,
    actor       VARCHAR(255) NOT NULL,
    old_value   JSONB,
    new_value   JSONB,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_api_key_events_prefix
    ON api_key_events (key_prefix, created_at);

-- migrate:down
DROP TABLE IF EXISTS api_key_events;
ALTER TABLE api_keys DROP COLUMN IF EXISTS disabled_at;
