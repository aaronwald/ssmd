-- migrate:up

-- Billing rates: per-endpoint-tier pricing with effective dates
-- key_prefix = NULL means global fallback rate
CREATE TABLE billing_rates (
    id              SERIAL PRIMARY KEY,
    key_prefix      VARCHAR(30),
    endpoint_tier   VARCHAR(64) NOT NULL,
    rate_per_request NUMERIC(12, 6) NOT NULL DEFAULT 0,
    rate_per_mb     NUMERIC(12, 6) NOT NULL DEFAULT 0,
    rate_per_1k_tokens NUMERIC(12, 6) NOT NULL DEFAULT 0,
    effective_from  TIMESTAMPTZ NOT NULL,
    effective_to    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (key_prefix, endpoint_tier, effective_from)
);

CREATE INDEX idx_billing_rates_lookup
    ON billing_rates (endpoint_tier, effective_from);

-- Seed global fallback rates (all USD)
INSERT INTO billing_rates (key_prefix, endpoint_tier, rate_per_request, rate_per_mb, rate_per_1k_tokens, effective_from) VALUES
    (NULL, 'data_query',     0.001,  0, 0, '2026-01-01T00:00:00Z'),
    (NULL, 'data_download',  0.005,  0.01, 0, '2026-01-01T00:00:00Z'),
    (NULL, 'secmaster',      0.0005, 0, 0, '2026-01-01T00:00:00Z'),
    (NULL, 'market_lookup',  0.0005, 0, 0, '2026-01-01T00:00:00Z'),
    (NULL, 'llm_chat',       0,      0, 0.03, '2026-01-01T00:00:00Z');

-- Billing ledger: credits and debits per key (all USD)
CREATE TABLE billing_ledger (
    id              BIGSERIAL PRIMARY KEY,
    key_prefix      VARCHAR(30) NOT NULL,
    entry_type      VARCHAR(16) NOT NULL CHECK (entry_type IN ('credit', 'debit')),
    amount_usd      NUMERIC(12, 6) NOT NULL CHECK (amount_usd > 0),
    description     TEXT NOT NULL,
    reference_month VARCHAR(7),
    actor           VARCHAR(255) NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_billing_ledger_key
    ON billing_ledger (key_prefix, created_at);

-- Add cost_usd to billing_daily_summary
ALTER TABLE billing_daily_summary
    ADD COLUMN cost_usd NUMERIC(12, 6) NOT NULL DEFAULT 0;

-- migrate:down
ALTER TABLE billing_daily_summary DROP COLUMN IF EXISTS cost_usd;
DROP TABLE IF EXISTS billing_ledger;
DROP TABLE IF EXISTS billing_rates;
