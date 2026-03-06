-- Make session_id nullable so WS broadcast events (market_settled, position_update,
-- connected, disconnected) can be recorded without a user session.
ALTER TABLE exchange_audit_log ALTER COLUMN session_id DROP NOT NULL;

INSERT INTO schema_migrations (version) VALUES ('014_nullable_audit_session_id')
    ON CONFLICT DO NOTHING;
