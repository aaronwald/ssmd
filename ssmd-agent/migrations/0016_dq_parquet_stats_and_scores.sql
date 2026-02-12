-- migrate:up
CREATE TABLE dq_parquet_stats (
  id SERIAL PRIMARY KEY,
  path TEXT NOT NULL,
  feed TEXT NOT NULL,
  msg_type TEXT NOT NULL,
  date DATE NOT NULL,
  rows INTEGER NOT NULL,
  file_size_bytes BIGINT NOT NULL,
  compression_ratio NUMERIC(8,4),
  duplicates_filtered INTEGER NOT NULL DEFAULT 0,
  schema_valid BOOLEAN NOT NULL DEFAULT true,
  null_violations JSONB NOT NULL DEFAULT '{}',
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_dq_parquet_stats_path ON dq_parquet_stats(path);
CREATE INDEX idx_dq_parquet_stats_feed_date ON dq_parquet_stats(feed, date);

ALTER TABLE dq_daily_scores
  ADD COLUMN gap_count INTEGER,
  ADD COLUMN gap_total_minutes NUMERIC(10,2),
  ADD COLUMN coverage_pct NUMERIC(5,2),
  ADD COLUMN expected_messages INTEGER,
  ADD COLUMN actual_messages INTEGER;

-- migrate:down
ALTER TABLE dq_daily_scores
  DROP COLUMN IF EXISTS gap_count,
  DROP COLUMN IF EXISTS gap_total_minutes,
  DROP COLUMN IF EXISTS coverage_pct,
  DROP COLUMN IF EXISTS expected_messages,
  DROP COLUMN IF EXISTS actual_messages;

DROP TABLE IF EXISTS dq_parquet_stats;
