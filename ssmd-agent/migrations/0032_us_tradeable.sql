-- migrate:up

-- Add US tradability columns to pairs table
ALTER TABLE pairs ADD COLUMN IF NOT EXISTS us_tradeable BOOLEAN;
ALTER TABLE pairs ADD COLUMN IF NOT EXISTS us_source VARCHAR(32);
ALTER TABLE pairs ADD COLUMN IF NOT EXISTS us_checked_at TIMESTAMPTZ;

-- Index for filtering by US tradability
CREATE INDEX IF NOT EXISTS idx_pairs_us_tradeable ON pairs (us_tradeable) WHERE us_tradeable IS NOT NULL;

-- migrate:down

DROP INDEX IF EXISTS idx_pairs_us_tradeable;
ALTER TABLE pairs DROP COLUMN IF EXISTS us_checked_at;
ALTER TABLE pairs DROP COLUMN IF EXISTS us_source;
ALTER TABLE pairs DROP COLUMN IF EXISTS us_tradeable;
