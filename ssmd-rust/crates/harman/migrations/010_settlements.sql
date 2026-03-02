-- Settlement records from exchange
CREATE TABLE IF NOT EXISTS settlements (
    id BIGSERIAL PRIMARY KEY,
    session_id BIGINT NOT NULL REFERENCES sessions(id),
    ticker TEXT NOT NULL,
    event_ticker TEXT NOT NULL,
    market_result TEXT NOT NULL CHECK (market_result IN ('yes', 'no', 'scalar', 'void')),
    yes_count NUMERIC(20,8) NOT NULL DEFAULT 0,
    no_count NUMERIC(20,8) NOT NULL DEFAULT 0,
    revenue_dollars NUMERIC(20,8) NOT NULL,
    settled_time TIMESTAMPTZ NOT NULL,
    fee_cost_dollars NUMERIC(20,8) NOT NULL DEFAULT 0,
    value_dollars NUMERIC(20,8),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(session_id, ticker)
);
CREATE INDEX IF NOT EXISTS idx_settlements_ticker ON settlements(ticker);
CREATE INDEX IF NOT EXISTS idx_settlements_session ON settlements(session_id);

INSERT INTO schema_migrations (version) VALUES ('010_settlements') ON CONFLICT DO NOTHING;
