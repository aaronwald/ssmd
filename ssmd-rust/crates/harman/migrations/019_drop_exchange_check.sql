-- Drop overly restrictive exchange name constraint.
-- Added in 006 as NOT VALID, never present in prod.
-- Prevents test sessions with dynamic exchange names and blocks adding new exchanges.
ALTER TABLE sessions DROP CONSTRAINT IF EXISTS sessions_exchange_check;

INSERT INTO schema_migrations (version) VALUES ('019_drop_exchange_check') ON CONFLICT DO NOTHING;
