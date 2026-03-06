-- migrate:up
ALTER TABLE prediction_orders
ADD CONSTRAINT valid_order_state CHECK (
    state IN ('pending', 'submitted', 'acknowledged', 'partially_filled',
              'filled', 'pending_cancel', 'pending_amend', 'pending_decrease',
              'cancelled', 'rejected', 'expired', 'staged', 'monitoring')
);

-- migrate:down
ALTER TABLE prediction_orders DROP CONSTRAINT valid_order_state;
