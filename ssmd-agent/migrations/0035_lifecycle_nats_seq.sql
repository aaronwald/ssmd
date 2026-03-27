-- migrate:up
ALTER TABLE market_lifecycle_events ADD COLUMN nats_seq BIGINT;
CREATE UNIQUE INDEX idx_mle_nats_seq ON market_lifecycle_events (nats_seq) WHERE nats_seq IS NOT NULL;

-- migrate:down
DROP INDEX IF EXISTS idx_mle_nats_seq;
ALTER TABLE market_lifecycle_events DROP COLUMN IF EXISTS nats_seq;
