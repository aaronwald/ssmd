-- Track which exchange environment (prod vs demo) the session connects to
ALTER TABLE sessions ADD COLUMN IF NOT EXISTS environment TEXT NOT NULL DEFAULT 'demo' CHECK (environment IN ('prod', 'demo'));

-- Human-readable identity for admin UIs
ALTER TABLE sessions ADD COLUMN IF NOT EXISTS display_name TEXT;

-- Drop old partial unique index
DROP INDEX IF EXISTS sessions_exchange_prefix_open;

-- New index includes environment
CREATE UNIQUE INDEX IF NOT EXISTS sessions_exchange_env_prefix_open
    ON sessions (exchange, environment, api_key_prefix)
    WHERE closed_at IS NULL AND api_key_prefix IS NOT NULL;

-- Startup session (null prefix): one per exchange+env
CREATE UNIQUE INDEX IF NOT EXISTS sessions_exchange_env_null_prefix_open
    ON sessions (exchange, environment)
    WHERE closed_at IS NULL AND api_key_prefix IS NULL;

-- Constrain exchange values (includes 'test' for test instances)
-- CRD uses 'kraken' but DB normalizes to 'kraken-futures' (feed name convention)
ALTER TABLE sessions ADD CONSTRAINT sessions_exchange_check
    CHECK (exchange IN ('kalshi', 'kraken-futures', 'polymarket', 'test'))
    NOT VALID;

-- Index for admin queries filtering by environment
CREATE INDEX IF NOT EXISTS idx_sessions_environment ON sessions(environment);

INSERT INTO schema_migrations (version) VALUES ('006_session_environment') ON CONFLICT DO NOTHING;
