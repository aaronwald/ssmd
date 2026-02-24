-- Harman MVP schema
-- Transactional outbox pattern for never-lose-a-transaction order management
-- Idempotent: safe to re-run on existing databases.

-- Sessions (hardcoded to 1 for MVP)
CREATE TABLE IF NOT EXISTS sessions (
    id BIGSERIAL PRIMARY KEY,
    exchange TEXT NOT NULL,
    actor TEXT NOT NULL DEFAULT 'system',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    closed_at TIMESTAMPTZ
);

-- Insert the default MVP session (idempotent)
INSERT INTO sessions (id, exchange) VALUES (1, 'kalshi') ON CONFLICT (id) DO NOTHING;

-- Orders
-- NOTE: side CHECK ('yes'/'no') is prediction-market-specific.
-- This table is intentionally scoped to prediction markets (Kalshi), not a generalized order table.
CREATE TABLE IF NOT EXISTS prediction_orders (
    id BIGSERIAL PRIMARY KEY,
    session_id BIGINT NOT NULL REFERENCES sessions(id),
    client_order_id UUID NOT NULL UNIQUE,
    exchange_order_id TEXT,
    ticker TEXT NOT NULL,
    side TEXT NOT NULL CHECK (side IN ('yes', 'no')),
    action TEXT NOT NULL CHECK (action IN ('buy', 'sell')),
    quantity INT NOT NULL CHECK (quantity > 0),
    price_cents INT NOT NULL CHECK (price_cents > 0 AND price_cents < 100),
    filled_quantity INT NOT NULL DEFAULT 0,
    time_in_force TEXT NOT NULL CHECK (time_in_force IN ('gtc', 'ioc')),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN (
        'pending', 'submitted', 'acknowledged', 'partially_filled',
        'filled', 'pending_cancel', 'cancelled', 'rejected', 'expired'
    )),
    cancel_reason TEXT CHECK (cancel_reason IN (
        'user_requested', 'risk_limit_breached', 'shutdown', 'expired', 'exchange_cancel'
    )),
    actor TEXT NOT NULL DEFAULT 'system',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_prediction_orders_state ON prediction_orders(state) WHERE state NOT IN ('filled', 'cancelled', 'rejected', 'expired');
CREATE INDEX IF NOT EXISTS idx_prediction_orders_session ON prediction_orders(session_id);

-- Order queue (transactional outbox)
-- Orders are inserted here atomically with the order itself.
-- The sweeper dequeues from here using SELECT FOR UPDATE SKIP LOCKED.
CREATE TABLE IF NOT EXISTS order_queue (
    id BIGSERIAL PRIMARY KEY,
    order_id BIGINT NOT NULL REFERENCES prediction_orders(id),
    action TEXT NOT NULL CHECK (action IN ('submit', 'cancel')),
    actor TEXT NOT NULL DEFAULT 'system',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processing BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX IF NOT EXISTS idx_order_queue_pending ON order_queue(id) WHERE NOT processing;

-- Fills (trade executions)
CREATE TABLE IF NOT EXISTS fills (
    id BIGSERIAL PRIMARY KEY,
    order_id BIGINT NOT NULL REFERENCES prediction_orders(id),
    trade_id TEXT NOT NULL UNIQUE,
    price_cents INT NOT NULL,
    quantity INT NOT NULL CHECK (quantity > 0),
    is_taker BOOLEAN NOT NULL DEFAULT FALSE,
    actor TEXT NOT NULL DEFAULT 'system',
    filled_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_fills_order ON fills(order_id);

-- Audit log for state transitions
CREATE TABLE IF NOT EXISTS audit_log (
    id BIGSERIAL PRIMARY KEY,
    order_id BIGINT NOT NULL REFERENCES prediction_orders(id),
    from_state TEXT NOT NULL,
    to_state TEXT NOT NULL,
    event TEXT NOT NULL,
    actor TEXT NOT NULL DEFAULT 'system',
    details JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_log_order ON audit_log(order_id);

-- Trigger to auto-update updated_at on prediction_orders
CREATE OR REPLACE FUNCTION update_prediction_orders_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Drop and recreate trigger (CREATE OR REPLACE not available for triggers)
DROP TRIGGER IF EXISTS prediction_orders_updated_at ON prediction_orders;
CREATE TRIGGER prediction_orders_updated_at
    BEFORE UPDATE ON prediction_orders
    FOR EACH ROW
    EXECUTE FUNCTION update_prediction_orders_updated_at();

-- NOTIFY trigger for order queue (future optimization, not used in MVP polling)
CREATE OR REPLACE FUNCTION notify_order_queue()
RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify('order_queue', NEW.id::TEXT);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS order_queue_notify ON order_queue;
CREATE TRIGGER order_queue_notify
    AFTER INSERT ON order_queue
    FOR EACH ROW
    EXECUTE FUNCTION notify_order_queue();
