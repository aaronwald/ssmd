-- migrate:up
-- Convert market price columns from INTEGER (cents 0-100) to NUMERIC (dollars 0.00-1.00).
-- Kalshi is deprecating integer cent fields on March 5 2026 in favor of _dollars fields.
-- Existing cent values are divided by 100 to convert to dollars.

-- Must drop dependent view first (instruments references yes_bid, yes_ask, last_price)
DROP VIEW IF EXISTS instruments;

ALTER TABLE markets
    ALTER COLUMN yes_bid TYPE NUMERIC(8, 4) USING (yes_bid::NUMERIC / 100),
    ALTER COLUMN yes_ask TYPE NUMERIC(8, 4) USING (yes_ask::NUMERIC / 100),
    ALTER COLUMN no_bid TYPE NUMERIC(8, 4) USING (no_bid::NUMERIC / 100),
    ALTER COLUMN no_ask TYPE NUMERIC(8, 4) USING (no_ask::NUMERIC / 100),
    ALTER COLUMN last_price TYPE NUMERIC(8, 4) USING (last_price::NUMERIC / 100);

-- Recreate instruments view with updated column types (no more /100 for Kalshi)
CREATE OR REPLACE VIEW instruments AS

-- Kalshi prediction markets (prices now in dollars, no division needed)
SELECT
    'kalshi' AS exchange,
    'prediction' AS instrument_type,
    m.ticker AS instrument_id,
    m.title AS name,
    e.category,
    m.status,
    m.yes_bid AS bid,
    m.yes_ask AS ask,
    m.last_price AS last_price,
    m.volume::numeric AS volume_24h,
    m.open_interest::numeric AS open_interest,
    m.close_time AS expiry,
    NULL::numeric AS funding_rate,
    NULL::numeric AS mark_price,
    NULL::numeric AS index_price,
    jsonb_build_object(
        'event_ticker', m.event_ticker,
        'series_ticker', e.series_ticker
    ) AS metadata,
    m.updated_at
FROM markets m
JOIN events e ON m.event_ticker = e.event_ticker
WHERE m.deleted_at IS NULL

UNION ALL

-- Kraken spot pairs
SELECT
    p.exchange,
    'spot' AS instrument_type,
    p.pair_id AS instrument_id,
    p.base || '/' || p.quote AS name,
    NULL AS category,
    p.status,
    p.bid::numeric AS bid,
    p.ask::numeric AS ask,
    p.last_price::numeric AS last_price,
    p.volume_24h::numeric AS volume_24h,
    NULL::numeric AS open_interest,
    NULL::timestamptz AS expiry,
    NULL::numeric AS funding_rate,
    NULL::numeric AS mark_price,
    NULL::numeric AS index_price,
    jsonb_build_object(
        'ws_name', p.ws_name,
        'tick_size', p.tick_size
    ) AS metadata,
    p.updated_at
FROM pairs p
WHERE p.market_type = 'spot'
  AND p.deleted_at IS NULL

UNION ALL

-- Kraken perpetual futures
SELECT
    p.exchange,
    'perpetual' AS instrument_type,
    p.pair_id AS instrument_id,
    p.base || '/' || p.quote || ' Perp' AS name,
    NULL AS category,
    CASE WHEN p.suspended THEN 'suspended'
         WHEN NOT p.tradeable THEN 'inactive'
         ELSE COALESCE(p.status, 'active')
    END AS status,
    p.bid::numeric AS bid,
    p.ask::numeric AS ask,
    p.last_price::numeric AS last_price,
    p.volume_24h::numeric AS volume_24h,
    p.open_interest::numeric AS open_interest,
    NULL::timestamptz AS expiry,
    p.funding_rate::numeric AS funding_rate,
    p.mark_price::numeric AS mark_price,
    p.index_price::numeric AS index_price,
    jsonb_build_object(
        'contract_type', p.contract_type,
        'underlying', p.underlying,
        'contract_size', p.contract_size,
        'funding_rate_prediction', p.funding_rate_prediction
    ) AS metadata,
    p.updated_at
FROM pairs p
WHERE p.market_type = 'perpetual'
  AND p.deleted_at IS NULL

UNION ALL

-- Polymarket prediction markets
SELECT
    'polymarket' AS exchange,
    'prediction' AS instrument_type,
    pc.condition_id AS instrument_id,
    pc.question AS name,
    pc.category,
    pc.status,
    NULL::numeric AS bid,
    NULL::numeric AS ask,
    NULL::numeric AS last_price,
    pc.volume::numeric AS volume_24h,
    NULL::numeric AS open_interest,
    pc.end_date AS expiry,
    NULL::numeric AS funding_rate,
    NULL::numeric AS mark_price,
    NULL::numeric AS index_price,
    jsonb_build_object(
        'slug', pc.slug,
        'outcomes', pc.outcomes
    ) AS metadata,
    pc.updated_at
FROM polymarket_conditions pc
WHERE pc.deleted_at IS NULL;

-- migrate:down
-- Drop the updated view, convert back to cents, recreate original view
DROP VIEW IF EXISTS instruments;

ALTER TABLE markets
    ALTER COLUMN yes_bid TYPE INT USING (ROUND(yes_bid * 100))::INT,
    ALTER COLUMN yes_ask TYPE INT USING (ROUND(yes_ask * 100))::INT,
    ALTER COLUMN no_bid TYPE INT USING (ROUND(no_bid * 100))::INT,
    ALTER COLUMN no_ask TYPE INT USING (ROUND(no_ask * 100))::INT,
    ALTER COLUMN last_price TYPE INT USING (ROUND(last_price * 100))::INT;

-- Recreate original instruments view with /100 division
CREATE OR REPLACE VIEW instruments AS
SELECT
    'kalshi' AS exchange,
    'prediction' AS instrument_type,
    m.ticker AS instrument_id,
    m.title AS name,
    e.category,
    m.status,
    m.yes_bid::numeric / 100.0 AS bid,
    m.yes_ask::numeric / 100.0 AS ask,
    m.last_price::numeric / 100.0 AS last_price,
    m.volume::numeric AS volume_24h,
    m.open_interest::numeric AS open_interest,
    m.close_time AS expiry,
    NULL::numeric AS funding_rate,
    NULL::numeric AS mark_price,
    NULL::numeric AS index_price,
    jsonb_build_object(
        'event_ticker', m.event_ticker,
        'series_ticker', e.series_ticker
    ) AS metadata,
    m.updated_at
FROM markets m
JOIN events e ON m.event_ticker = e.event_ticker
WHERE m.deleted_at IS NULL
UNION ALL
SELECT
    p.exchange,
    'spot' AS instrument_type,
    p.pair_id AS instrument_id,
    p.base || '/' || p.quote AS name,
    NULL AS category,
    p.status,
    p.bid::numeric AS bid,
    p.ask::numeric AS ask,
    p.last_price::numeric AS last_price,
    p.volume_24h::numeric AS volume_24h,
    NULL::numeric AS open_interest,
    NULL::timestamptz AS expiry,
    NULL::numeric AS funding_rate,
    NULL::numeric AS mark_price,
    NULL::numeric AS index_price,
    jsonb_build_object(
        'ws_name', p.ws_name,
        'tick_size', p.tick_size
    ) AS metadata,
    p.updated_at
FROM pairs p
WHERE p.market_type = 'spot'
  AND p.deleted_at IS NULL
UNION ALL
SELECT
    p.exchange,
    'perpetual' AS instrument_type,
    p.pair_id AS instrument_id,
    p.base || '/' || p.quote || ' Perp' AS name,
    NULL AS category,
    CASE WHEN p.suspended THEN 'suspended'
         WHEN NOT p.tradeable THEN 'inactive'
         ELSE COALESCE(p.status, 'active')
    END AS status,
    p.bid::numeric AS bid,
    p.ask::numeric AS ask,
    p.last_price::numeric AS last_price,
    p.volume_24h::numeric AS volume_24h,
    p.open_interest::numeric AS open_interest,
    NULL::timestamptz AS expiry,
    p.funding_rate::numeric AS funding_rate,
    p.mark_price::numeric AS mark_price,
    p.index_price::numeric AS index_price,
    jsonb_build_object(
        'contract_type', p.contract_type,
        'underlying', p.underlying,
        'contract_size', p.contract_size,
        'funding_rate_prediction', p.funding_rate_prediction
    ) AS metadata,
    p.updated_at
FROM pairs p
WHERE p.market_type = 'perpetual'
  AND p.deleted_at IS NULL
UNION ALL
SELECT
    'polymarket' AS exchange,
    'prediction' AS instrument_type,
    pc.condition_id AS instrument_id,
    pc.question AS name,
    pc.category,
    pc.status,
    NULL::numeric AS bid,
    NULL::numeric AS ask,
    NULL::numeric AS last_price,
    pc.volume::numeric AS volume_24h,
    NULL::numeric AS open_interest,
    pc.end_date AS expiry,
    NULL::numeric AS funding_rate,
    NULL::numeric AS mark_price,
    NULL::numeric AS index_price,
    jsonb_build_object(
        'slug', pc.slug,
        'outcomes', pc.outcomes
    ) AS metadata,
    pc.updated_at
FROM polymarket_conditions pc
WHERE pc.deleted_at IS NULL;
