-- Per-session daily loss limit override.
-- NULL = use global default from --daily-loss-limit / DAILY_LOSS_LIMIT env var.
ALTER TABLE sessions ADD COLUMN IF NOT EXISTS daily_loss_limit NUMERIC(20,8);

INSERT INTO schema_migrations (version) VALUES ('013_daily_loss_limit')
    ON CONFLICT DO NOTHING;
