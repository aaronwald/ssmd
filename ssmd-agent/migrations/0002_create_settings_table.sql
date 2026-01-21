-- migrate:up
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value JSONB NOT NULL,
  updated_at TIMESTAMPTZ DEFAULT NOW()
);

COMMENT ON TABLE settings IS 'Key-value store for application settings (e.g., guardrails config)';

-- migrate:down
DROP TABLE IF EXISTS settings;
