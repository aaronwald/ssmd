-- migrate:up

ALTER TABLE polymarket_conditions ADD COLUMN tags TEXT[] NOT NULL DEFAULT '{}';

-- GIN index for efficient array overlap/contains queries
CREATE INDEX idx_polymarket_conditions_tags ON polymarket_conditions USING GIN (tags) WHERE deleted_at IS NULL;

-- migrate:down

DROP INDEX IF EXISTS idx_polymarket_conditions_tags;
ALTER TABLE polymarket_conditions DROP COLUMN IF EXISTS tags;
