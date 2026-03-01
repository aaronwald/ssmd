-- Per-session risk limits (Phase 3)
-- NULL = use global default from --max-notional / MAX_NOTIONAL env var
ALTER TABLE sessions ADD COLUMN IF NOT EXISTS max_notional NUMERIC(20,8);

INSERT INTO schema_migrations (version) VALUES ('007_session_risk') ON CONFLICT DO NOTHING;
