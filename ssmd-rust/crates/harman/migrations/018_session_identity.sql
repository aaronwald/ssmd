-- Session identity: (exchange, environment) — one session per exchange per env.
-- api_key_prefix becomes mutable metadata, not part of the natural key.
-- Prerequisites: merge duplicate sessions before applying.
-- To apply: scale harman to 0, run migration, scale back up.

BEGIN;

DROP INDEX IF EXISTS sessions_natural_key;
ALTER TABLE sessions DROP CONSTRAINT IF EXISTS sessions_api_key_prefix_not_sentinel;
CREATE UNIQUE INDEX sessions_natural_key ON sessions (exchange, environment);
ALTER TABLE sessions ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

INSERT INTO schema_migrations (version) VALUES ('018_session_identity') ON CONFLICT DO NOTHING;

COMMIT;
