-- migrate:up
-- Add enrichment fields to polymarket_conditions for multi-outcome grouping,
-- negative risk flag, description, and order sizing constraints.

ALTER TABLE polymarket_conditions
    ADD COLUMN event_id VARCHAR(32),
    ADD COLUMN neg_risk BOOLEAN,
    ADD COLUMN description TEXT,
    ADD COLUMN order_price_min_tick_size NUMERIC(8, 6),
    ADD COLUMN order_min_size NUMERIC(12, 2);

-- Index on event_id for multi-outcome grouping queries
CREATE INDEX idx_polymarket_conditions_event_id ON polymarket_conditions (event_id) WHERE event_id IS NOT NULL;

-- migrate:down
DROP INDEX IF EXISTS idx_polymarket_conditions_event_id;

ALTER TABLE polymarket_conditions
    DROP COLUMN IF EXISTS event_id,
    DROP COLUMN IF EXISTS neg_risk,
    DROP COLUMN IF EXISTS description,
    DROP COLUMN IF EXISTS order_price_min_tick_size,
    DROP COLUMN IF EXISTS order_min_size;
