-- Migration 017: PriceMonitor support
-- Adds trigger_price for stop-loss price monitoring and order_type for IOC submission

ALTER TABLE prediction_orders ADD COLUMN IF NOT EXISTS trigger_price NUMERIC(20,10);
ALTER TABLE prediction_orders ADD COLUMN IF NOT EXISTS order_type TEXT NOT NULL DEFAULT 'limit';

-- Index for PriceMonitor recovery: find all armed triggers on startup
CREATE INDEX IF NOT EXISTS idx_orders_monitoring ON prediction_orders (state) WHERE state = 'monitoring';
