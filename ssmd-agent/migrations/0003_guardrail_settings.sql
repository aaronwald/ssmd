-- Migration: Add guardrail settings
-- Run with: kubectl exec -n ssmd deployment/ssmd-data-ts -- psql $DATABASE_URL -f -

-- Add description column if it doesn't exist
ALTER TABLE settings ADD COLUMN IF NOT EXISTS description TEXT;

-- Guardrail settings
INSERT INTO settings (key, value, description) VALUES
  ('guardrail_toxicity_enabled', 'true', 'Enable toxicity detection in agent output'),
  ('guardrail_hallucination_enabled', 'true', 'Enable hallucination detection in agent output'),
  ('guardrail_trading_approval', 'true', 'Require human approval for trading tool calls'),
  ('guardrail_max_messages', '50', 'Maximum messages to keep in context window')
ON CONFLICT (key) DO NOTHING;
