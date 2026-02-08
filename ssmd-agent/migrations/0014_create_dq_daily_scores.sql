-- migrate:up
CREATE TABLE dq_daily_scores (
  id SERIAL PRIMARY KEY,
  check_date DATE NOT NULL,
  feed TEXT NOT NULL,
  score NUMERIC(5,2) NOT NULL,
  composite_score NUMERIC(5,2),
  details JSONB NOT NULL DEFAULT '{}',
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_dq_daily_feed_date ON dq_daily_scores(check_date, feed);

-- migrate:down
DROP TABLE IF EXISTS dq_daily_scores;
