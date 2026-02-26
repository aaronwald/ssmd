-- Add amend and decrease support to harman OMS.
-- Extends order_queue actions, prediction_orders states, and adds metadata column.

-- Extend order_queue action CHECK to include 'amend' and 'decrease'
ALTER TABLE order_queue DROP CONSTRAINT IF EXISTS order_queue_action_check;
ALTER TABLE order_queue ADD CONSTRAINT order_queue_action_check
    CHECK (action IN ('submit', 'cancel', 'amend', 'decrease'));

-- Extend prediction_orders state CHECK to include 'pending_amend' and 'pending_decrease'
ALTER TABLE prediction_orders DROP CONSTRAINT IF EXISTS prediction_orders_state_check;
ALTER TABLE prediction_orders ADD CONSTRAINT prediction_orders_state_check
    CHECK (state IN (
        'pending', 'submitted', 'acknowledged', 'partially_filled',
        'filled', 'pending_cancel', 'pending_amend', 'pending_decrease',
        'cancelled', 'rejected', 'expired'
    ));

-- Add metadata JSONB column to order_queue for amend/decrease params
ALTER TABLE order_queue ADD COLUMN IF NOT EXISTS metadata JSONB;

INSERT INTO schema_migrations (version) VALUES ('003_amend_decrease');
