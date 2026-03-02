-- Stable sessions: replace ephemeral session IDs with permanent identity.
-- Demo/test only — no production data exists.
-- Scale harman to 0 replicas before running; scale up after.

BEGIN;

-- Wipe all transactional data (demo/test only)
TRUNCATE fills, audit_log, order_queue, order_groups, prediction_orders;
DELETE FROM sessions;

-- Sessions are permanent — drop lifecycle columns
ALTER TABLE sessions DROP COLUMN IF EXISTS closed_at;
ALTER TABLE sessions DROP COLUMN IF EXISTS actor;

-- Drop old partial unique indexes (scoped to open sessions)
DROP INDEX IF EXISTS sessions_exchange_env_prefix_open;
DROP INDEX IF EXISTS sessions_exchange_env_null_prefix_open;

-- Natural key: one session per (exchange, env, key_prefix)
-- COALESCE handles NULL api_key_prefix for startup sessions
CREATE UNIQUE INDEX sessions_natural_key
    ON sessions (exchange, environment, COALESCE(api_key_prefix, '__none__'));

-- Guard against sentinel value collision
ALTER TABLE sessions ADD CONSTRAINT sessions_api_key_prefix_not_sentinel
    CHECK (api_key_prefix IS DISTINCT FROM '__none__');

INSERT INTO schema_migrations (version) VALUES ('009_stable_sessions') ON CONFLICT DO NOTHING;

COMMIT;
