-- Add 'staged' to prediction_orders state CHECK
ALTER TABLE prediction_orders DROP CONSTRAINT IF EXISTS prediction_orders_state_check;
ALTER TABLE prediction_orders ADD CONSTRAINT prediction_orders_state_check
    CHECK (state IN (
        'pending', 'submitted', 'acknowledged', 'partially_filled',
        'filled', 'pending_cancel', 'pending_amend', 'pending_decrease',
        'cancelled', 'rejected', 'expired', 'staged'
    ));

-- Order groups
CREATE TABLE IF NOT EXISTS order_groups (
    id BIGSERIAL PRIMARY KEY,
    session_id BIGINT NOT NULL REFERENCES sessions(id),
    group_type TEXT NOT NULL CHECK (group_type IN ('bracket', 'oco')),
    state TEXT NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'completed', 'cancelled')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_order_groups_session ON order_groups(session_id);
CREATE INDEX IF NOT EXISTS idx_order_groups_active ON order_groups(state) WHERE state = 'active';

-- Link orders to groups
ALTER TABLE prediction_orders ADD COLUMN IF NOT EXISTS group_id BIGINT REFERENCES order_groups(id);
ALTER TABLE prediction_orders ADD COLUMN IF NOT EXISTS leg_role TEXT
    CHECK (leg_role IN ('entry', 'take_profit', 'stop_loss', 'oco_leg'));

CREATE INDEX IF NOT EXISTS idx_prediction_orders_group
    ON prediction_orders(group_id) WHERE group_id IS NOT NULL;

-- updated_at trigger for order_groups
CREATE OR REPLACE FUNCTION update_order_groups_updated_at()
RETURNS TRIGGER AS $$
BEGIN NEW.updated_at = NOW(); RETURN NEW; END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS order_groups_updated_at ON order_groups;
CREATE TRIGGER order_groups_updated_at
    BEFORE UPDATE ON order_groups FOR EACH ROW
    EXECUTE FUNCTION update_order_groups_updated_at();

INSERT INTO schema_migrations (version) VALUES ('005_order_groups')
    ON CONFLICT DO NOTHING;
