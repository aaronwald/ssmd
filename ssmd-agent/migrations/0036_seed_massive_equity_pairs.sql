-- migrate:up
-- Seed the static 85-ticker US-equity universe for the Massive (Polygon.io) feed.
-- pair_id uses "massive:<TICKER>" prefix to guarantee no collision with other exchange
-- pair_ids (Kraken spot uses bare currency-pair format; Kraken futures uses PF_/PI_ prefix).
INSERT INTO pairs (pair_id, exchange, base, quote, ws_name, market_type, status)
SELECT 'massive:' || t AS pair_id, 'massive', t AS base, 'USD', t AS ws_name, 'equity', 'active'
FROM unnest(ARRAY[
  'QUAL','QQQ','PDBC','TSLA','META','XLB','MCHI','EWJ','EWQ','IEMG','EFA','USMV','DBA','VIXY',
  'NFLX','UUP','BWX','XLV','CORN','MSFT','USO','VWO','GLD','VGSH','MTUM','KWEB','EMB','XLF','EEM',
  'XLU','XLI','TLT','VXZ','SLV','VIXM','VEA','IGOV','AAPL','BIL','RINF','XLE','FXI','SHY','DBC',
  'EWC','LQD','NVDA','FXY','IEF','XLY','KBE','JNK','PALL','XLP','IWM','ISHG','DXJ','DBV','BNO',
  'EMLC','ACWI','TUA','KRE','TIP','GOOGL','IVOL','CPER','GOVT','VLUE','HYG','INDA','EWU','XLK',
  'EWZ','USDU','VGK','VGIT','VT','AMZN','EWG','FXE','GSG','XLC','VXX','SPY'
]) AS t
ON CONFLICT (pair_id) DO UPDATE
  SET exchange    = EXCLUDED.exchange,
      base        = EXCLUDED.base,
      quote       = EXCLUDED.quote,
      ws_name     = EXCLUDED.ws_name,
      market_type = EXCLUDED.market_type,
      status      = 'active',
      updated_at  = NOW();

-- migrate:down
DELETE FROM pairs WHERE exchange = 'massive' AND market_type = 'equity';
