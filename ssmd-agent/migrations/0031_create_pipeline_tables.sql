-- migrate:up

-- Read-only role for SQL stage execution (defense in depth)
DO $$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'pipeline_readonly') THEN
    CREATE ROLE pipeline_readonly WITH LOGIN PASSWORD 'CHANGE_ME_AT_DEPLOY';
  END IF;
END
$$;
GRANT CONNECT ON DATABASE ssmd TO pipeline_readonly;
GRANT USAGE ON SCHEMA public TO pipeline_readonly;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO pipeline_readonly;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO pipeline_readonly;

CREATE TABLE pipeline_definitions (
  id              SERIAL PRIMARY KEY,
  name            TEXT NOT NULL UNIQUE CHECK (length(name) BETWEEN 1 AND 255),
  description     TEXT,
  trigger_type    TEXT NOT NULL CHECK (trigger_type IN ('webhook', 'cron')),
  trigger_config  JSONB NOT NULL DEFAULT '{}',
  webhook_secret_hash TEXT,
  enabled         BOOLEAN NOT NULL DEFAULT true,
  last_triggered_at TIMESTAMPTZ,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE pipeline_stages (
  id              SERIAL PRIMARY KEY,
  pipeline_id     INTEGER NOT NULL REFERENCES pipeline_definitions(id) ON DELETE CASCADE,
  position        INTEGER NOT NULL CHECK (position >= 0),
  name            TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 255),
  stage_type      TEXT NOT NULL CHECK (stage_type IN ('sql', 'http', 'gcs_check', 'openrouter', 'email')),
  config          JSONB NOT NULL DEFAULT '{}',
  UNIQUE(pipeline_id, position)
);

CREATE TABLE pipeline_runs (
  id              SERIAL PRIMARY KEY,
  pipeline_id     INTEGER NOT NULL REFERENCES pipeline_definitions(id) ON DELETE RESTRICT,
  status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'completed', 'failed')),
  trigger_info    JSONB,
  started_at      TIMESTAMPTZ,
  finished_at     TIMESTAMPTZ,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE pipeline_stage_results (
  id              SERIAL PRIMARY KEY,
  run_id          INTEGER NOT NULL REFERENCES pipeline_runs(id) ON DELETE CASCADE,
  stage_id        INTEGER REFERENCES pipeline_stages(id) ON DELETE SET NULL,
  status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'completed', 'failed')),
  input           JSONB,
  output          JSONB,
  error           TEXT,
  started_at      TIMESTAMPTZ,
  finished_at     TIMESTAMPTZ
);

-- Indexes
CREATE INDEX idx_pipeline_runs_pending ON pipeline_runs(created_at) WHERE status = 'pending';
CREATE INDEX idx_pipeline_runs_pipeline ON pipeline_runs(pipeline_id, created_at DESC);
CREATE INDEX idx_pipeline_stage_results_run ON pipeline_stage_results(run_id);
CREATE INDEX idx_pipeline_stages_pipeline ON pipeline_stages(pipeline_id, position);

-- updated_at trigger for pipeline_definitions
CREATE OR REPLACE FUNCTION pipeline_definitions_update_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER pipeline_definitions_updated_at_trigger
    BEFORE UPDATE ON pipeline_definitions
    FOR EACH ROW
    EXECUTE FUNCTION pipeline_definitions_update_timestamp();

-- migrate:down

DROP TRIGGER IF EXISTS pipeline_definitions_updated_at_trigger ON pipeline_definitions;
DROP FUNCTION IF EXISTS pipeline_definitions_update_timestamp();
DROP TABLE IF EXISTS pipeline_stage_results;
DROP TABLE IF EXISTS pipeline_runs;
DROP TABLE IF EXISTS pipeline_stages;
DROP TABLE IF EXISTS pipeline_definitions;
DROP ROLE IF EXISTS pipeline_readonly;
