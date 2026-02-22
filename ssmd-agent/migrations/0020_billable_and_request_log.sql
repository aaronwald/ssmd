-- migrate:up

-- Add billable flag to api_keys (default true for new keys)
ALTER TABLE api_keys ADD COLUMN billable BOOLEAN NOT NULL DEFAULT true;

-- Mark existing internal/service keys as non-billable
UPDATE api_keys SET billable = false WHERE rate_limit_tier = 'internal';
UPDATE api_keys SET billable = false WHERE user_email LIKE '%@ssmd.local';

-- General-purpose API request log for billing
CREATE TABLE api_request_log (
    id              BIGSERIAL PRIMARY KEY,
    key_prefix      VARCHAR(30) NOT NULL,
    method          VARCHAR(10) NOT NULL,
    path            VARCHAR(255) NOT NULL,
    status_code     SMALLINT NOT NULL,
    response_bytes  INTEGER,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Composite index for per-key monthly billing aggregation
CREATE INDEX idx_api_request_log_billing
    ON api_request_log (key_prefix, created_at);

-- For time-range scans (daily/monthly reports)
CREATE INDEX idx_api_request_log_created
    ON api_request_log (created_at);

-- migrate:down
DROP TABLE IF EXISTS api_request_log;
ALTER TABLE api_keys DROP COLUMN IF EXISTS billable;
