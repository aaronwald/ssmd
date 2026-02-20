-- migrate:up
CREATE INDEX idx_events_strike_date ON events(strike_date) WHERE deleted_at IS NULL;

-- migrate:down
DROP INDEX IF EXISTS idx_events_strike_date;
