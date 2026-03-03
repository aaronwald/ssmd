-- Exchange audit log: durable record of every REST call, WS event, fallback decision,
-- reconciliation action, recovery step, and risk check.
CREATE TABLE IF NOT EXISTS exchange_audit_log (
    id          BIGSERIAL PRIMARY KEY,
    session_id  BIGINT NOT NULL REFERENCES sessions(id),
    order_id    BIGINT REFERENCES prediction_orders(id),
    category    TEXT NOT NULL,
    action      TEXT NOT NULL,
    endpoint    TEXT,
    status_code INT,
    duration_ms INT,
    request     JSONB,
    response    JSONB,
    outcome     TEXT NOT NULL,
    error_msg   TEXT,
    metadata    JSONB,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_exchange_audit_session ON exchange_audit_log(session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_exchange_audit_order   ON exchange_audit_log(order_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_exchange_audit_cat     ON exchange_audit_log(category, created_at DESC);

INSERT INTO schema_migrations (version) VALUES ('011_exchange_audit_log') ON CONFLICT DO NOTHING;
