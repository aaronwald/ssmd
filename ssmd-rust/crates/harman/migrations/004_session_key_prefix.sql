-- Add api_key_prefix to sessions for per-key session isolation.
ALTER TABLE sessions ADD COLUMN IF NOT EXISTS api_key_prefix TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS sessions_exchange_prefix_open
  ON sessions (exchange, api_key_prefix) WHERE closed_at IS NULL AND api_key_prefix IS NOT NULL;

INSERT INTO schema_migrations (version) VALUES ('004_session_key_prefix')
  ON CONFLICT DO NOTHING;
