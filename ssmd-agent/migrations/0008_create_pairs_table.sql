-- migrate:up
CREATE TABLE pairs (
    pair_id VARCHAR(32) PRIMARY KEY,
    exchange VARCHAR(32) NOT NULL,
    base VARCHAR(16) NOT NULL,
    quote VARCHAR(16) NOT NULL,
    ws_name VARCHAR(32) NOT NULL,
    bid NUMERIC(18,8),
    ask NUMERIC(18,8),
    last_price NUMERIC(18,8),
    volume_24h NUMERIC(24,8),
    status VARCHAR(16) DEFAULT 'active',
    lot_decimals INTEGER DEFAULT 8,
    pair_decimals INTEGER DEFAULT 1,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_pairs_exchange ON pairs(exchange);
CREATE INDEX idx_pairs_base_quote ON pairs(base, quote);

-- Reuse existing trigger function from migration 0004
CREATE TRIGGER update_pairs_updated_at
    BEFORE UPDATE ON pairs
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

-- Seed initial pairs
INSERT INTO pairs (pair_id, exchange, base, quote, ws_name) VALUES
    ('BTCUSD', 'kraken', 'BTC', 'USD', 'BTC/USD'),
    ('ETHUSD', 'kraken', 'ETH', 'USD', 'ETH/USD');

-- migrate:down
DROP TABLE IF EXISTS pairs;
