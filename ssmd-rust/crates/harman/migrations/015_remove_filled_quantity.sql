-- Remove filled_quantity shadow field from prediction_orders.
-- The fills table is the source of truth. Derive via filled_qty() function.

-- SQL function: returns SUM(fills.quantity) for an order, or 0 if no fills.
CREATE OR REPLACE FUNCTION filled_qty(p_order_id bigint)
RETURNS numeric(18,8) AS $$
    SELECT COALESCE(SUM(quantity), 0) FROM fills WHERE order_id = p_order_id;
$$ LANGUAGE SQL STABLE;

-- Drop the shadow column
ALTER TABLE prediction_orders DROP COLUMN filled_quantity;

INSERT INTO schema_migrations (version) VALUES ('015_remove_filled_quantity')
    ON CONFLICT DO NOTHING;
