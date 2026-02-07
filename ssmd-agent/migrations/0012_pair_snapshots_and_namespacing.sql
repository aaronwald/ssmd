-- migrate:up

-- Namespace existing pair_ids with exchange prefix (kraken:XXBTZUSD instead of XXBTZUSD)
-- This prevents PK collisions when adding Binance/Coinbase pairs
UPDATE pairs SET pair_id = exchange || ':' || pair_id
WHERE pair_id NOT LIKE '%:%';

-- Create pair_snapshots for time-series perpetual data
-- (markPrice, fundingRate, etc. were being overwritten each sync - no history)
CREATE TABLE pair_snapshots (
    id BIGSERIAL PRIMARY KEY,
    pair_id VARCHAR(128) NOT NULL REFERENCES pairs(pair_id),
    mark_price NUMERIC(18,8),
    index_price NUMERIC(18,8),
    funding_rate NUMERIC(18,12),
    funding_rate_prediction NUMERIC(18,12),
    open_interest NUMERIC(24,8),
    last_price NUMERIC(18,8),
    bid NUMERIC(18,8),
    ask NUMERIC(18,8),
    volume_24h NUMERIC(24,8),
    suspended BOOLEAN DEFAULT false,
    snapshot_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_pair_snapshots_pair_time ON pair_snapshots(pair_id, snapshot_at DESC);
CREATE INDEX idx_pair_snapshots_time ON pair_snapshots(snapshot_at DESC);

-- Recreate instruments view (pair_id values changed with namespace prefix)
DROP VIEW IF EXISTS instruments;
CREATE OR REPLACE VIEW instruments AS

-- Kalshi prediction markets
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
DROP TABLE IF EXISTS pair_snapshots;
UPDATE pairs SET pair_id = SUBSTRING(pair_id FROM POSITION(':' IN pair_id) + 1)
WHERE pair_id LIKE '%:%';
DROP VIEW IF EXISTS instruments;
CREATE OR REPLACE VIEW instruments AS

-- Kalshi prediction markets
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
