-- migrate:up

-- Pre-aggregated daily billing data (populated by billing aggregate CronJob)
CREATE TABLE billing_daily_summary (
    id              BIGSERIAL PRIMARY KEY,
    key_prefix      VARCHAR(30) NOT NULL,
    date            DATE NOT NULL,
    endpoint        VARCHAR(255) NOT NULL,
    request_count   INTEGER NOT NULL DEFAULT 0,
    response_bytes  BIGINT NOT NULL DEFAULT 0,
    error_count     INTEGER NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (key_prefix, date, endpoint)
);

CREATE INDEX idx_billing_daily_key_date
    ON billing_daily_summary (key_prefix, date);

-- migrate:down
DROP TABLE IF EXISTS billing_daily_summary;
