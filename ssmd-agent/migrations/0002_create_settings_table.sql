-- Migration: Create settings table for guardrail configuration
-- Run with: kubectl exec -n ssmd deployment/ssmd-data-ts -- psql $DATABASE_URL -f -

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value JSONB NOT NULL,
  updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Add comment for documentation
COMMENT ON TABLE settings IS 'Key-value store for application settings (e.g., guardrails config)';
