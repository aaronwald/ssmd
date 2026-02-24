-- Schema migration tracking
CREATE TABLE IF NOT EXISTS schema_migrations (
    version TEXT PRIMARY KEY,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- prediction_orders: price_cents INT → price_dollars NUMERIC(18,8)
ALTER TABLE prediction_orders
    ALTER COLUMN quantity TYPE NUMERIC(18,8),
    ALTER COLUMN price_cents TYPE NUMERIC(18,8) USING (price_cents::NUMERIC / 100),
    ALTER COLUMN filled_quantity TYPE NUMERIC(18,8);
ALTER TABLE prediction_orders RENAME COLUMN price_cents TO price_dollars;

-- Update CHECK constraint for price (drop old, add new)
ALTER TABLE prediction_orders DROP CONSTRAINT IF EXISTS prediction_orders_price_cents_check;
ALTER TABLE prediction_orders ADD CONSTRAINT prediction_orders_price_dollars_check
    CHECK (price_dollars > 0 AND price_dollars < 1);

-- fills: same treatment
ALTER TABLE fills
    ALTER COLUMN price_cents TYPE NUMERIC(18,8) USING (price_cents::NUMERIC / 100),
    ALTER COLUMN quantity TYPE NUMERIC(18,8);
ALTER TABLE fills RENAME COLUMN price_cents TO price_dollars;

INSERT INTO schema_migrations (version) VALUES ('002_decimal_migration');
