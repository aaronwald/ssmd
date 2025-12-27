-- migrations/001_secmaster.sql
-- Kalshi secmaster schema

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

CREATE INDEX idx_events_category ON events(category) WHERE deleted_at IS NULL;
CREATE INDEX idx_events_series ON events(series_ticker) WHERE deleted_at IS NULL;
CREATE INDEX idx_events_status ON events(status) WHERE deleted_at IS NULL;

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

CREATE INDEX idx_markets_event ON markets(event_ticker) WHERE deleted_at IS NULL;
CREATE INDEX idx_markets_status ON markets(status) WHERE deleted_at IS NULL;
CREATE INDEX idx_markets_close_time ON markets(close_time) WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS fees (
    tier VARCHAR(32) PRIMARY KEY,
    maker_fee DECIMAL(6,4) NOT NULL,
    taker_fee DECIMAL(6,4) NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Insert default fee tier
INSERT INTO fees (tier, maker_fee, taker_fee) VALUES ('default', 0.07, 0.07)
ON CONFLICT (tier) DO NOTHING;
