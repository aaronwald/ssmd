-- migrate:up
-- Initial schema: events, markets, fees, series, series_fees, api_keys
-- Consolidated from root migrations/ (001_secmaster, 002_series, 002_series_fees, 003_api_keys)

-- === Secmaster (001_secmaster) ===

CREATE TABLE IF NOT EXISTS events (
    event_ticker VARCHAR(64) PRIMARY KEY,
    title TEXT NOT NULL,
    category VARCHAR(64) NOT NULL DEFAULT '',
    series_ticker VARCHAR(64) NOT NULL DEFAULT '',
    strike_date TIMESTAMPTZ,
    mutually_exclusive BOOLEAN NOT NULL DEFAULT false,
    status VARCHAR(16) NOT NULL DEFAULT 'open',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_events_category ON events(category) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_events_series ON events(series_ticker) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_events_status ON events(status) WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS markets (
    ticker VARCHAR(64) PRIMARY KEY,
    event_ticker VARCHAR(64) NOT NULL REFERENCES events(event_ticker),
    title TEXT NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'open',
    close_time TIMESTAMPTZ,
    yes_bid INT,
    yes_ask INT,
    no_bid INT,
    no_ask INT,
    last_price INT,
    volume BIGINT,
    volume_24h BIGINT,
    open_interest BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_markets_event ON markets(event_ticker) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_markets_status ON markets(status) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_markets_close_time ON markets(close_time) WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS fees (
    tier VARCHAR(32) PRIMARY KEY,
    maker_fee DECIMAL(6,4) NOT NULL,
    taker_fee DECIMAL(6,4) NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO fees (tier, maker_fee, taker_fee) VALUES ('default', 0.07, 0.07)
ON CONFLICT (tier) DO NOTHING;

-- === Series (002_series) ===

CREATE TABLE IF NOT EXISTS series (
    ticker VARCHAR(64) PRIMARY KEY,
    title TEXT NOT NULL,
    category VARCHAR(64) NOT NULL,
    tags TEXT[],
    is_game BOOLEAN NOT NULL DEFAULT false,
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_series_category ON series(category) WHERE active = true;
CREATE INDEX IF NOT EXISTS idx_series_category_game ON series(category, is_game) WHERE active = true;
CREATE INDEX IF NOT EXISTS idx_series_tags ON series USING GIN(tags) WHERE active = true;

-- === Series Fees (002_series_fees) ===

CREATE EXTENSION IF NOT EXISTS btree_gist;

DO $$ BEGIN
    CREATE TYPE fee_type AS ENUM ('quadratic', 'quadratic_with_maker_fees', 'flat');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS series_fees (
    id SERIAL PRIMARY KEY,
    series_ticker VARCHAR(64) NOT NULL,
    fee_type fee_type NOT NULL,
    fee_multiplier NUMERIC(6,4) NOT NULL DEFAULT 1.0,
    effective_from TIMESTAMPTZ NOT NULL,
    effective_to TIMESTAMPTZ,
    source_id VARCHAR(128),
    created_at TIMESTAMPTZ DEFAULT NOW(),

    CONSTRAINT no_overlapping_fees EXCLUDE USING gist (
        series_ticker WITH =,
        tstzrange(effective_from, COALESCE(effective_to, 'infinity'::timestamptz), '[)') WITH &&
    )
);

CREATE INDEX IF NOT EXISTS idx_series_fees_ticker ON series_fees(series_ticker);
CREATE INDEX IF NOT EXISTS idx_series_fees_effective ON series_fees(effective_from, effective_to);
CREATE UNIQUE INDEX IF NOT EXISTS idx_series_fees_source_id ON series_fees(source_id) WHERE source_id IS NOT NULL;

COMMENT ON TABLE series_fees IS 'Kalshi series fee schedules with time-travel support. Use effective_from/to for point-in-time queries.';
COMMENT ON COLUMN series_fees.fee_type IS 'quadratic=taker only, quadratic_with_maker_fees=both, flat=per-contract';
COMMENT ON COLUMN series_fees.fee_multiplier IS 'Series-specific multiplier applied to base fee formula';
COMMENT ON COLUMN series_fees.effective_to IS 'NULL means currently active, set when superseded';
COMMENT ON COLUMN series_fees.source_id IS 'Kalshi API fee change ID for deduplication';

-- === API Keys (003_api_keys) ===

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

CREATE INDEX IF NOT EXISTS idx_api_keys_prefix ON api_keys(key_prefix) WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);

-- migrate:down
DROP TABLE IF EXISTS api_keys;
DROP TABLE IF EXISTS series_fees;
DROP TYPE IF EXISTS fee_type;
DROP TABLE IF EXISTS series;
DROP TABLE IF EXISTS markets;
DROP TABLE IF EXISTS events;
DROP TABLE IF EXISTS fees;
