-- migrate:up
ALTER TABLE sessions ADD COLUMN IF NOT EXISTS api_key_prefix TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS sessions_exchange_prefix_open
  ON sessions (exchange, api_key_prefix) WHERE closed_at IS NULL AND api_key_prefix IS NOT NULL;

INSERT INTO schema_migrations (version) VALUES ('004_session_key_prefix')
  ON CONFLICT DO NOTHING;

-- migrate:down
DROP INDEX IF EXISTS sessions_exchange_prefix_open;
ALTER TABLE sessions DROP COLUMN IF EXISTS api_key_prefix;
DELETE FROM schema_migrations WHERE version = '004_session_key_prefix';
