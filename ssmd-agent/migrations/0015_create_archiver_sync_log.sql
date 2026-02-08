-- migrate:up
CREATE TABLE archiver_sync_log (
  id SERIAL PRIMARY KEY,
  archiver_name TEXT NOT NULL,
  synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  success BOOLEAN NOT NULL DEFAULT true,
  duration_ms INTEGER,
  details JSONB NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_archiver_sync_log_name_time
  ON archiver_sync_log(archiver_name, synced_at DESC);

-- migrate:down
DROP TABLE archiver_sync_log;
