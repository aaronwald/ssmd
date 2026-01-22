-- migrate:up
CREATE TABLE market_lifecycle_events (
    id BIGSERIAL PRIMARY KEY,
    market_ticker VARCHAR(128) NOT NULL,
    event_type VARCHAR(32) NOT NULL,  -- created, activated, deactivated, close_date_updated, determined, settled
    open_ts TIMESTAMPTZ,
    close_ts TIMESTAMPTZ,
    settled_ts TIMESTAMPTZ,
    metadata JSONB,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_mle_market ON market_lifecycle_events(market_ticker);
CREATE INDEX idx_mle_event_type ON market_lifecycle_events(event_type);
CREATE INDEX idx_mle_received ON market_lifecycle_events(received_at);

COMMENT ON TABLE market_lifecycle_events IS 'Kalshi market lifecycle events from market_lifecycle_v2 channel';
COMMENT ON COLUMN market_lifecycle_events.event_type IS 'created, activated, deactivated, close_date_updated, determined, settled';

-- migrate:down
DROP TABLE market_lifecycle_events;
