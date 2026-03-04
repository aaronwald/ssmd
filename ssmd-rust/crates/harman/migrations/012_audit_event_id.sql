-- Add event_id for idempotent audit inserts (retry-safe deduplication).
ALTER TABLE exchange_audit_log ADD COLUMN event_id UUID;
CREATE UNIQUE INDEX IF NOT EXISTS idx_exchange_audit_event_id ON exchange_audit_log(event_id) WHERE event_id IS NOT NULL;

INSERT INTO schema_migrations (version) VALUES ('012_audit_event_id') ON CONFLICT DO NOTHING;
