-- migrate:up

-- Widen pair_id for multi-exchange support (Kraken futures IDs fit in 32, but
-- other exchanges may use longer identifiers — match polymarket_conditions at 128)
ALTER TABLE pairs ALTER COLUMN pair_id TYPE VARCHAR(128);

-- Market type discriminator (spot or perpetual)
ALTER TABLE pairs ADD COLUMN market_type VARCHAR(16) NOT NULL DEFAULT 'spot';

-- Spot-specific fields (Kraken AssetPairs API)
ALTER TABLE pairs ADD COLUMN altname VARCHAR(32);
ALTER TABLE pairs ADD COLUMN tick_size NUMERIC(18,10);
ALTER TABLE pairs ADD COLUMN order_min NUMERIC(18,8);
ALTER TABLE pairs ADD COLUMN cost_min NUMERIC(18,8);
ALTER TABLE pairs ADD COLUMN fee_schedule JSONB;

-- Perpetual-specific fields (Kraken Futures /instruments + /tickers)
ALTER TABLE pairs ADD COLUMN underlying VARCHAR(32);
ALTER TABLE pairs ADD COLUMN contract_size NUMERIC(18,8);
ALTER TABLE pairs ADD COLUMN contract_type VARCHAR(32);
ALTER TABLE pairs ADD COLUMN mark_price NUMERIC(18,8);
ALTER TABLE pairs ADD COLUMN index_price NUMERIC(18,8);
ALTER TABLE pairs ADD COLUMN funding_rate NUMERIC(18,12);
ALTER TABLE pairs ADD COLUMN funding_rate_prediction NUMERIC(18,12);
ALTER TABLE pairs ADD COLUMN open_interest NUMERIC(24,8);
ALTER TABLE pairs ADD COLUMN max_position_size NUMERIC(24,8);
ALTER TABLE pairs ADD COLUMN margin_levels JSONB;
ALTER TABLE pairs ADD COLUMN tradeable BOOLEAN DEFAULT true;
ALTER TABLE pairs ADD COLUMN suspended BOOLEAN DEFAULT false;
ALTER TABLE pairs ADD COLUMN opening_date TIMESTAMPTZ;
ALTER TABLE pairs ADD COLUMN fee_schedule_uid VARCHAR(64);
ALTER TABLE pairs ADD COLUMN tags TEXT[];
ALTER TABLE pairs ADD COLUMN deleted_at TIMESTAMPTZ;

-- Indexes
CREATE INDEX idx_pairs_market_type ON pairs(market_type);
CREATE INDEX idx_pairs_exchange_type ON pairs(exchange, market_type);
CREATE INDEX idx_pairs_active ON pairs(exchange, market_type) WHERE deleted_at IS NULL AND status = 'active';

-- Upgrade updated_at trigger to IS DISTINCT FROM pattern (matches events/markets from 0004)
CREATE OR REPLACE FUNCTION pairs_update_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    IF (OLD.bid, OLD.ask, OLD.last_price, OLD.volume_24h, OLD.status,
        OLD.mark_price, OLD.index_price, OLD.funding_rate, OLD.funding_rate_prediction,
        OLD.open_interest, OLD.tradeable, OLD.suspended, OLD.deleted_at)
       IS DISTINCT FROM
       (NEW.bid, NEW.ask, NEW.last_price, NEW.volume_24h, NEW.status,
        NEW.mark_price, NEW.index_price, NEW.funding_rate, NEW.funding_rate_prediction,
        NEW.open_interest, NEW.tradeable, NEW.suspended, NEW.deleted_at)
    THEN
        NEW.updated_at = NOW();
    ELSE
        NEW.updated_at = OLD.updated_at;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Seed BTC and ETH perpetual pairs
INSERT INTO pairs (pair_id, exchange, base, quote, ws_name, market_type, contract_type) VALUES
    ('PF_XBTUSD', 'kraken', 'BTC', 'USD', 'PF_XBTUSD', 'perpetual', 'flexible_futures'),
    ('PF_ETHUSD', 'kraken', 'ETH', 'USD', 'PF_ETHUSD', 'perpetual', 'flexible_futures'),
    ('PI_XBTUSD', 'kraken', 'BTC', 'USD', 'PI_XBTUSD', 'perpetual', 'futures_inverse'),
    ('PI_ETHUSD', 'kraken', 'ETH', 'USD', 'PI_ETHUSD', 'perpetual', 'futures_inverse');

-- migrate:down
-- Drop seeded perp pairs
DELETE FROM pairs WHERE market_type = 'perpetual';

-- Drop indexes
DROP INDEX IF EXISTS idx_pairs_active;
DROP INDEX IF EXISTS idx_pairs_exchange_type;
DROP INDEX IF EXISTS idx_pairs_market_type;

-- Restore simple trigger
CREATE OR REPLACE FUNCTION pairs_update_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Drop added columns (reverse order)
ALTER TABLE pairs DROP COLUMN IF EXISTS deleted_at;
ALTER TABLE pairs DROP COLUMN IF EXISTS tags;
ALTER TABLE pairs DROP COLUMN IF EXISTS fee_schedule_uid;
ALTER TABLE pairs DROP COLUMN IF EXISTS opening_date;
ALTER TABLE pairs DROP COLUMN IF EXISTS suspended;
ALTER TABLE pairs DROP COLUMN IF EXISTS tradeable;
ALTER TABLE pairs DROP COLUMN IF EXISTS margin_levels;
ALTER TABLE pairs DROP COLUMN IF EXISTS max_position_size;
ALTER TABLE pairs DROP COLUMN IF EXISTS open_interest;
ALTER TABLE pairs DROP COLUMN IF EXISTS funding_rate_prediction;
ALTER TABLE pairs DROP COLUMN IF EXISTS funding_rate;
ALTER TABLE pairs DROP COLUMN IF EXISTS index_price;
ALTER TABLE pairs DROP COLUMN IF EXISTS mark_price;
ALTER TABLE pairs DROP COLUMN IF EXISTS contract_type;
ALTER TABLE pairs DROP COLUMN IF EXISTS contract_size;
ALTER TABLE pairs DROP COLUMN IF EXISTS underlying;
ALTER TABLE pairs DROP COLUMN IF EXISTS fee_schedule;
ALTER TABLE pairs DROP COLUMN IF EXISTS cost_min;
ALTER TABLE pairs DROP COLUMN IF EXISTS order_min;
ALTER TABLE pairs DROP COLUMN IF EXISTS tick_size;
ALTER TABLE pairs DROP COLUMN IF EXISTS altname;
ALTER TABLE pairs DROP COLUMN IF EXISTS market_type;

-- Restore original pair_id width
ALTER TABLE pairs ALTER COLUMN pair_id TYPE VARCHAR(32);
