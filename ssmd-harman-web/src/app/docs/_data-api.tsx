import { Endpoint, Section } from "./_components";

// ===========================================================================
// Market Data API (api.varshtat.com)
//
// This is the PUBLIC data + secmaster API — distinct from the Harman OMS API
// documented above. Base URL: https://api.varshtat.com. Auth: an API key
// passed as `X-API-Key: <key>` (or `Authorization: Bearer <key>`), NOT the
// harman session token. Scopes: `datasets:read` (data endpoints) and
// `secmaster:read` (metadata endpoints). All content verified against the
// data-ts route handlers.
// ===========================================================================

export function DataApiSections() {
  return (
    <>
      {/* ===================================================================== */}
      {/* OVERVIEW */}
      {/* ===================================================================== */}
      <div id="market-data-api" className="scroll-mt-20 pt-4">
        <h2 className="text-xl font-bold text-fg mb-2 border-b border-border pb-2">
          Market Data API
        </h2>
        <div className="text-sm text-fg-muted space-y-3 mb-6">
          <p>
            The public data + secmaster API, served at{" "}
            <code className="font-mono text-accent">https://api.varshtat.com</code>. This is a
            separate service from the Harman OMS API above — it has its own base URL, auth, and
            scopes.
          </p>
          <div className="border border-border rounded p-3 bg-bg">
            <p className="font-semibold text-fg text-sm">Authentication</p>
            <p className="text-xs text-fg-muted mt-1">
              Pass your key as <code className="font-mono text-accent">X-API-Key: &lt;key&gt;</code>{" "}
              or <code className="font-mono text-accent">Authorization: Bearer &lt;key&gt;</code> on
              every request. Examples below use{" "}
              <code className="font-mono text-accent">$API_KEY</code>.
            </p>
          </div>
          <div className="border border-border rounded p-3 bg-bg">
            <p className="font-semibold text-fg text-sm">Scopes</p>
            <div className="text-xs text-fg-muted mt-1 space-y-1">
              <p>
                <code className="font-mono text-accent">datasets:read</code> — data endpoints
                (trades, prices, events, volume, freshness, snap, ohlcv, download, catalog).
              </p>
              <p>
                <code className="font-mono text-accent">secmaster:read</code> — metadata endpoints
                (events, markets, series, pairs, fees, stats).
              </p>
              <p>
                Keys may also be restricted to specific feeds and a date range; download/data
                endpoints enforce and clamp to those limits server-side. Request access via{" "}
                <a
                  href="https://github.com/aaronwald/ssmd/issues"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-accent hover:underline"
                >
                  GitHub Issues
                </a>
                .
              </p>
            </div>
          </div>
          <div className="border border-border rounded p-3 bg-bg">
            <p className="font-semibold text-fg text-sm">Feeds</p>
            <p className="text-xs text-fg-muted mt-1">
              Raw-archive catalog feeds (<code className="font-mono text-accent">/v1/data/feeds</code>):{" "}
              <code className="font-mono text-accent">kalshi</code> (crypto markets),{" "}
              <code className="font-mono text-accent">kraken-spot</code> (crypto),{" "}
              <code className="font-mono text-accent">massive</code> (US equities, ~15-min delayed), and{" "}
              <code className="font-mono text-accent">kraken-futures</code> (archived — decommissioned
              2026-06-03, historical data only). The derived{" "}
              <code className="font-mono text-accent">hols</code> daily crypto OHLCV dataset is not in
              that catalog — fetch it via{" "}
              <code className="font-mono text-accent">/v1/data/download?feed=hols</code>. DuckDB query
              endpoints (trades/prices/events/volume/freshness/snap) accept{" "}
              <code className="font-mono text-accent">kalshi</code>,{" "}
              <code className="font-mono text-accent">kraken-spot</code>,{" "}
              <code className="font-mono text-accent">massive</code> only.
            </p>
          </div>
          <div className="border border-border rounded p-3 bg-bg">
            <p className="font-semibold text-fg text-sm">Schemas</p>
            <p className="text-xs text-fg-muted mt-1">
              Message &amp; parquet schema reference (ssmd repo):{" "}
              <a
                href="https://github.com/aaronwald/ssmd/blob/main/docs/schemas/parquet-schemas.md"
                target="_blank"
                rel="noopener noreferrer"
                className="text-accent hover:underline"
              >
                Parquet schemas
              </a>
              ,{" "}
              <a
                href="https://github.com/aaronwald/ssmd/blob/main/docs/schemas/kalshi-json.md"
                target="_blank"
                rel="noopener noreferrer"
                className="text-accent hover:underline"
              >
                Kalshi JSON
              </a>
              ,{" "}
              <a
                href="https://github.com/aaronwald/ssmd/blob/main/docs/schemas/kraken-futures-json.md"
                target="_blank"
                rel="noopener noreferrer"
                className="text-accent hover:underline"
              >
                Kraken Futures JSON
              </a>
              ,{" "}
              <a
                href="https://github.com/aaronwald/ssmd/blob/main/docs/schemas/polymarket-json.md"
                target="_blank"
                rel="noopener noreferrer"
                className="text-accent hover:underline"
              >
                Polymarket JSON
              </a>
              ,{" "}
              <a
                href="https://github.com/aaronwald/ssmd/blob/main/docs/schemas/binance-json.md"
                target="_blank"
                rel="noopener noreferrer"
                className="text-accent hover:underline"
              >
                Binance JSON
              </a>
              . Current per-feed schema versions are served live at{" "}
              <code className="font-mono text-accent">/v1/data/schema-versions</code> (see{" "}
              <a href="#data-endpoints" className="text-accent hover:underline">
                Data
              </a>
              ). The{" "}
              <code className="font-mono text-accent">/v1/data/ohlcv/1m</code> bar schema is a
              derived product (not in the parquet/schema-versions registry); it is documented
              separately in{" "}
              <a
                href="https://github.com/aaronwald/ssmd/blob/main/docs/schemas/ohlcv-bars.md"
                target="_blank"
                rel="noopener noreferrer"
                className="text-accent hover:underline"
              >
                1m OHLCV bars
              </a>
              .
            </p>
          </div>
          <div className="border border-border rounded p-3 bg-bg">
            <p className="font-semibold text-fg text-sm">Errors &amp; rate limits</p>
            <p className="text-xs text-fg-muted mt-1">
              Errors return <code className="font-mono text-accent">{`{ "error": "..." }`}</code> with
              the relevant status: <code className="font-mono text-accent">400</code> bad request,{" "}
              <code className="font-mono text-accent">401</code> missing/invalid key,{" "}
              <code className="font-mono text-accent">403</code> key lacks scope/feed/date access,{" "}
              <code className="font-mono text-accent">404</code> not found,{" "}
              <code className="font-mono text-accent">429</code> rate limited,{" "}
              <code className="font-mono text-accent">503</code> upstream unavailable. Default rate
              limit is 60 requests/min per IP (Cloud Armor).
            </p>
          </div>
        </div>
      </div>

      {/* ===================================================================== */}
      {/* SECMASTER */}
      {/* ===================================================================== */}
      <Section id="secmaster" title="Secmaster (Metadata)">
        <Endpoint
          method="GET"
          path="/v1/secmaster/stats"
          scope="secmaster:read"
          description="Aggregate counts for events, markets, pairs, and conditions across all exchanges."
          response={`{
  "events": { "total": 1500, "by_status": { "active": 1200, "settled": 300 }, "by_category": { "Crypto": 900, "Sports": 600 } },
  "markets": { "total": 350000, "by_status": { "active": 40000, "settled": 310000 } },
  "pairs": { "total": 820, "by_exchange": { "kraken": 820 }, "by_market_type": { "spot": 500, "perpetual": 320 } },
  "conditions": { "total": 9841 }
}`}
          curl={`curl "https://api.varshtat.com/v1/secmaster/stats" \\
  -H "X-API-Key: $API_KEY"`}
        />
        <Endpoint
          method="GET"
          path="/v1/events"
          scope="secmaster:read"
          description="List Kalshi events. All filters optional."
          queryParams={[
            { name: "category", description: "Filter by category (e.g. Crypto, Sports)" },
            { name: "series", description: "Filter by series ticker (e.g. KXBTCD)" },
            { name: "status", description: "Filter by status (e.g. active, settled)" },
            { name: "as_of", description: "ISO 8601 timestamp — point-in-time view" },
            { name: "limit", description: "Max results (default 100)" },
          ]}
          response={`{
  "events": [
    {
      "eventTicker": "KXBTCD-26JUN2611",
      "title": "Bitcoin price on Jun 26 11AM EST",
      "category": "Crypto",
      "seriesTicker": "KXBTCD",
      "status": "active",
      "strikeDate": "2026-06-26T15:00:00.000Z",
      "mutuallyExclusive": true,
      "marketCount": 24,
      "createdAt": "2026-06-25T12:00:00.000Z",
      "updatedAt": "2026-06-26T10:00:00.000Z"
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/events?status=active&category=Crypto&limit=50" \\
  -H "X-API-Key: $API_KEY"`}
          notes="GET /v1/events/:ticker returns a single event (404 if not found)."
        />
        <Endpoint
          method="GET"
          path="/v1/markets"
          scope="secmaster:read"
          description="List markets with optional filters. Prices are decimal strings (dollars)."
          queryParams={[
            { name: "category", description: "Filter by category" },
            { name: "series", description: "Filter by series ticker" },
            { name: "event", description: "Filter by event ticker" },
            { name: "status", description: "Filter by status" },
            { name: "close_within_hours", description: "Markets closing within N hours" },
            { name: "closing_before / closing_after", description: "ISO 8601 close-time bounds" },
            { name: "open_before", description: "ISO 8601 — markets opened on/before this time" },
            { name: "games_only", description: "true to restrict to sports/game markets" },
            { name: "as_of", description: "ISO 8601 timestamp — point-in-time view" },
            { name: "limit", description: "Max results (default 100)" },
            { name: "include_snapshot", description: "true to add CDC sync metadata to the envelope" },
          ]}
          response={`{
  "markets": [
    {
      "ticker": "KXBTCD-26JUN2611-B105000",
      "eventTicker": "KXBTCD-26JUN2611",
      "title": "Bitcoin above $105,000",
      "status": "active",
      "closeTime": "2026-06-26T15:00:00.000Z",
      "yesBid": "0.45", "yesAsk": "0.47", "noBid": "0.53", "noAsk": "0.55",
      "lastPrice": "0.46",
      "volume": 12030, "volume24h": 8800, "openInterest": 4521,
      "marketType": "binary", "canCloseEarly": true,
      "openTime": "2026-06-25T15:00:00.000Z",
      "expectedExpirationTime": "2026-06-26T15:00:00.000Z"
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/markets?series=KXBTCD&status=active&close_within_hours=2" \\
  -H "X-API-Key: $API_KEY"`}
          notes="GET /v1/markets/:ticker returns a single market (404 if not found). Price columns are NUMERIC, serialized as strings; volume fields are integers."
        />
        <Endpoint
          method="GET"
          path="/v1/markets/lookup"
          scope="datasets:read"
          description="Cross-feed lookup by identifier (Kalshi tickers, Kraken pair_ids, Polymarket condition/token IDs)."
          queryParams={[
            { name: "ids", description: "Comma-separated IDs, max 100 (required)" },
            { name: "feed", description: "Optional feed hint: kalshi, kraken-futures, kraken-spot, polymarket, massive" },
          ]}
          response={`{
  "markets": [
    { "feed": "kalshi", "id": "KXBTCD-26JUN2611-B105000", "title": "Bitcoin above $105,000", "status": "active" }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/markets/lookup?ids=KXBTCD-26JUN2611-B105000" \\
  -H "X-API-Key: $API_KEY"`}
        />
        <Endpoint
          method="GET"
          path="/v1/series"
          scope="secmaster:read"
          description="List series with optional filters."
          queryParams={[
            { name: "category", description: "Filter by category" },
            { name: "tag", description: "Filter by tag" },
            { name: "games_only", description: "true to restrict to sports/game series" },
            { name: "limit", description: "Max results" },
          ]}
          response={`{
  "series": [
    {
      "ticker": "KXBTCD",
      "title": "Bitcoin Daily",
      "category": "Crypto",
      "tags": ["crypto", "btc"],
      "isGame": false,
      "active": true,
      "volume": 1500000
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/series?category=Crypto" \\
  -H "X-API-Key: $API_KEY"`}
        />
      </Section>

      {/* ===================================================================== */}
      {/* KRAKEN */}
      {/* ===================================================================== */}
      <Section id="kraken-pairs" title="Kraken Pairs">
        <Endpoint
          method="GET"
          path="/v1/pairs"
          scope="secmaster:read"
          description="List Kraken pairs (spot + futures/perpetual)."
          queryParams={[
            { name: "exchange", description: "Filter by exchange" },
            { name: "market_type", description: "perpetual, monthly, or spot" },
            { name: "base", description: "Base currency (e.g. XBT, ETH)" },
            { name: "quote", description: "Quote currency (e.g. USD)" },
            { name: "status", description: "Filter by status" },
            { name: "limit", description: "Max results (default 100)" },
          ]}
          response={`{
  "pairs": [
    {
      "pairId": "kraken:PF_XBTUSD",
      "exchange": "kraken",
      "base": "XBT", "quote": "USD",
      "wsName": "PF_XBTUSD",
      "marketType": "perpetual",
      "status": "active",
      "markPrice": "105000.0", "indexPrice": "104990.0",
      "fundingRate": "0.00001", "openInterest": "1234.5",
      "lastPrice": "105010.0", "bid": "104995.0", "ask": "105005.0",
      "volume24h": "500.0", "tradeable": true
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/pairs?market_type=perpetual&base=XBT&status=active" \\
  -H "X-API-Key: $API_KEY"`}
          notes="GET /v1/pairs/:pairId returns a single pair (404 if not found)."
        />
        <Endpoint
          method="GET"
          path="/v1/pairs/:pairId/snapshots"
          scope="secmaster:read"
          description="Historical mark/index/funding snapshots for a pair, newest first."
          queryParams={[
            { name: "from", description: "ISO 8601 start time" },
            { name: "to", description: "ISO 8601 end time" },
            { name: "limit", description: "Max results (default 100)" },
          ]}
          response={`{
  "snapshots": [
    {
      "id": 123,
      "pairId": "kraken:PF_XBTUSD",
      "markPrice": "105000.0", "indexPrice": "104990.0",
      "fundingRate": "0.00001", "openInterest": "1234.5",
      "lastPrice": "105010.0", "bid": "104995.0", "ask": "105005.0",
      "volume24h": "500.0",
      "snapshotAt": "2026-06-26T10:00:00.000Z"
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/pairs/kraken:PF_XBTUSD/snapshots?limit=10" \\
  -H "X-API-Key: $API_KEY"`}
        />
      </Section>

      {/* ===================================================================== */}
      {/* FEES */}
      {/* ===================================================================== */}
      <Section id="fees" title="Fees">
        <Endpoint
          method="GET"
          path="/v1/fees"
          scope="secmaster:read"
          description="Current fee schedules for all Kalshi series."
          queryParams={[{ name: "limit", description: "Max results (default 100)" }]}
          response={`{
  "fees": [
    {
      "id": 12,
      "seriesTicker": "KXBTCD",
      "feeType": "percentage",
      "feeMultiplier": "0.07",
      "effectiveFrom": "2026-01-01T00:00:00.000Z",
      "effectiveTo": null
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/fees" \\
  -H "X-API-Key: $API_KEY"`}
        />
        <Endpoint
          method="GET"
          path="/v1/fees/:series"
          scope="secmaster:read"
          description="Fee schedule for a single series. Without as_of returns the current fee."
          queryParams={[{ name: "as_of", description: "ISO 8601 timestamp — historical fee lookup" }]}
          response={`{
  "id": 12,
  "seriesTicker": "KXBTCD",
  "feeType": "percentage",
  "feeMultiplier": "0.07",
  "effectiveFrom": "2026-01-01T00:00:00.000Z",
  "effectiveTo": null
}`}
          curl={`curl "https://api.varshtat.com/v1/fees/KXBTCD?as_of=2026-02-01T00:00:00Z" \\
  -H "X-API-Key: $API_KEY"`}
          notes="404 if no fee schedule exists for the series."
        />
      </Section>

      {/* ===================================================================== */}
      {/* DATA ENDPOINTS */}
      {/* ===================================================================== */}
      <Section id="data-endpoints" title="Data">
        <Endpoint
          method="GET"
          path="/v1/data/feeds"
          scope="datasets:read"
          description="List available raw-archive data feeds with date coverage and row counts (from the GCS catalog). Returns kalshi, kraken-futures, kraken-spot, and massive. The derived hols daily-OHLCV dataset is not a catalog feed — fetch it via /v1/data/download?feed=hols."
          response={`{
  "feeds": [
    {
      "name": "kalshi",
      "prefix": "kalshi",
      "stream": "crypto",
      "messageTypes": ["ticker", "trade"],
      "dateMin": "2026-02-17",
      "dateMax": "2026-06-28",
      "totalFiles": 3023,
      "totalRows": 123739764
    }
  ],
  "catalogGeneratedAt": "2026-06-28T12:30:47.000Z"
}`}
          curl={`curl "https://api.varshtat.com/v1/data/feeds" \\
  -H "X-API-Key: $API_KEY"`}
        />
        <Endpoint
          method="GET"
          path="/v1/data/catalog"
          scope="datasets:read"
          description="Catalog of available parquet data. Omit feed for an overview; pass feed for the per-date file list. Filtered to your key's authorized feeds."
          queryParams={[
            { name: "feed", description: "Single feed for a detailed per-date listing" },
            { name: "from / to", description: "YYYY-MM-DD bounds on the dates list" },
          ]}
          response={`{
  "feed": "hols",
  "from": "2026-06-01",
  "to": "2026-06-26",
  "dates": [
    { "date": "2026-06-26", "messageTypes": ["ohlcv"] }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/data/catalog?feed=hols&from=2026-06-01&to=2026-06-26" \\
  -H "X-API-Key: $API_KEY"`}
        />
        <Endpoint
          method="GET"
          path="/v1/data/trades"
          scope="datasets:read"
          description="Per-ticker trade summary for a feed/date (DuckDB over GCS parquet)."
          queryParams={[
            { name: "feed", description: "kalshi, kraken-spot, or massive (required)" },
            { name: "date", description: "YYYY-MM-DD (default today)" },
            { name: "limit", description: "1–1000 (default 20)" },
          ]}
          response={`{
  "feed": "kalshi",
  "date": "2026-06-26",
  "count": 20,
  "trades": [
    {
      "ticker": "KXBTCD-26JUN2611-B105000",
      "trade_count": 840,
      "total_volume": 12030,
      "min_price": 0.40, "max_price": 0.55, "avg_price": 0.46
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/data/trades?feed=kalshi&date=2026-06-26&limit=100" \\
  -H "X-API-Key: $API_KEY"`}
          notes="Kalshi prices are in dollars (cents ÷ 100). Defaults to today's date if omitted."
        />
        <Endpoint
          method="GET"
          path="/v1/data/prices"
          scope="datasets:read"
          description="Latest ticker price snapshot per market for a feed/date."
          queryParams={[
            { name: "feed", description: "kalshi, kraken-spot, or massive (required)" },
            { name: "date", description: "YYYY-MM-DD (default today)" },
            { name: "hour", description: "Optional HHMM (e.g. 1400)" },
          ]}
          response={`{
  "feed": "kalshi",
  "date": "2026-06-26",
  "hour": "1400",
  "count": 350,
  "prices": [
    {
      "ticker": "KXBTCD-26JUN2611-B105000",
      "yes_bid": 0.45, "yes_ask": 0.47, "no_bid": 0.53, "no_ask": 0.55,
      "last_price": 0.46, "volume": 12030, "open_interest": 4521,
      "ts": "2026-06-26T14:00:00Z"
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/data/prices?feed=kalshi&date=2026-06-26&hour=1400" \\
  -H "X-API-Key: $API_KEY"`}
        />
        <Endpoint
          method="GET"
          path="/v1/data/events"
          scope="datasets:read"
          description="Per-event volume rollup (kalshi only). Aggregates trades by event with top markets."
          queryParams={[
            { name: "feed", description: "kalshi (required)" },
            { name: "date", description: "YYYY-MM-DD (default today)" },
            { name: "limit", description: "1–100 (default 20)" },
          ]}
          response={`{
  "feed": "kalshi",
  "date": "2026-06-26",
  "volumeUnit": "contracts",
  "count": 20,
  "events": [
    {
      "eventId": "KXBTCD-26JUN2611",
      "totalTradeCount": 5400,
      "totalVolume": 98000,
      "marketCount": 24,
      "metadata": { "title": "Bitcoin price on Jun 26 11AM EST", "category": "Crypto", "status": "active" },
      "topMarkets": [ { "ticker": "KXBTCD-26JUN2611-B105000", "tradeCount": 840, "volume": 12030 } ]
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/data/events?feed=kalshi&date=2026-06-26" \\
  -H "X-API-Key: $API_KEY"`}
          notes="Only the kalshi feed is supported; other feeds return 500."
        />
        <Endpoint
          method="GET"
          path="/v1/data/volume"
          scope="datasets:read"
          description="Daily volume summary. Omit feed for all authorized feeds, or pass one."
          queryParams={[
            { name: "feed", description: "Optional: kalshi, kraken-spot, or massive" },
            { name: "date", description: "YYYY-MM-DD (default today)" },
          ]}
          response={`{
  "date": "2026-06-26",
  "feeds": [
    {
      "feed": "kalshi",
      "totalTradeCount": 120000,
      "totalVolume": 2400000,
      "volumeUnit": "contracts",
      "activeTickers": 3500,
      "topTickers": [ { "ticker": "KXBTCD-26JUN2611-B105000", "tradeCount": 840, "volume": 12030 } ]
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/data/volume?date=2026-06-26" \\
  -H "X-API-Key: $API_KEY"`}
          notes="volumeUnit is absent for kraken-spot. Per-feed failures are reported inline as { feed, error } rather than failing the whole response."
        />
        <Endpoint
          method="GET"
          path="/v1/data/freshness"
          scope="datasets:read"
          description="Data freshness per feed. Stale threshold is 7 hours."
          queryParams={[{ name: "feed", description: "Optional: omit to check all data feeds" }]}
          response={`{
  "checked_at": "2026-06-26T10:00:00.000Z",
  "stale_threshold_hours": 7,
  "feeds": [
    {
      "feed": "kalshi",
      "status": "fresh",
      "newest_date": "2026-06-26",
      "newest_hour": "0900",
      "age_hours": 1.0,
      "stale": false
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/data/freshness?feed=kalshi" \\
  -H "X-API-Key: $API_KEY"`}
          notes="Per-feed status may also be no_data, unknown, or error."
        />
        <Endpoint
          method="GET"
          path="/v1/data/snap"
          scope="datasets:read"
          description="Live price snapshots from Redis (ssmd-snap)."
          queryParams={[
            { name: "feed", description: "kalshi, kraken-spot, or massive (required)" },
            { name: "tickers", description: "Optional comma-separated list (max 500); omit to scan up to 500" },
          ]}
          response={`{
  "feed": "kalshi",
  "count": 1,
  "snapshots": [
    {
      "_ticker": "KXBTCD-26JUN2611-B105000",
      "yes_bid": 0.45, "yes_ask": 0.47, "no_bid": 0.53, "no_ask": 0.55,
      "last_price": 0.46, "volume": 12030, "ts": "2026-06-26T10:00:00Z"
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/data/snap?feed=kalshi&tickers=KXBTCD-26JUN2611-B105000" \\
  -H "X-API-Key: $API_KEY"`}
        />
        <Endpoint
          method="GET"
          path="/v1/data/ohlcv/1m"
          scope="datasets:read"
          description="Live 1-minute OHLCV bars from a rolling ~60-minute cache, updated continuously. Includes trade counts and aggressor-side volume splits."
          queryParams={[
            { name: "feed", description: "massive (US equities/ETFs), kraken-spot (crypto), or binance (crypto) (required)" },
            { name: "sym", description: "Symbol: AAPL or SPY (massive); BTC/USDT or ETH/USDT (kraken-spot); BTCUSDT or ETHUSDT (binance) (required)" },
            { name: "limit", description: "Most-recent bars to return, 1–60 (default 60)" },
          ]}
          response={`{
  "feed": "binance",
  "sym": "BTCUSDT",
  "bars": [
    {
      "sym": "BTCUSDT",
      "o": 42500.5, "h": 42750.0, "l": 42400.0, "c": 42625.3,
      "v": 128.45,
      "trade_count": 1205,
      "taker_buy_volume": 72.3, "taker_sell_volume": 56.15,
      "market_order_volume": 0.0,
      "quote_volume": 5458125.0,
      "start_ts_ms": 1782124200000,
      "end_ts_ms": 1782124260000
    }
  ],
  "served_at": "2026-06-27T14:30:15.842Z"
}`}
          curl={`# US equities/ETFs (massive)
curl "https://api.varshtat.com/v1/data/ohlcv/1m?feed=massive&sym=AAPL&limit=5" \\
  -H "X-API-Key: $API_KEY"

# Crypto (kraken-spot) — URL-encode the slash in the pair (BTC/USDT -> BTC%2FUSDT)
curl "https://api.varshtat.com/v1/data/ohlcv/1m?feed=kraken-spot&sym=BTC%2FUSDT&limit=5" \\
  -H "X-API-Key: $API_KEY"

# Crypto (binance) — no slash in symbols
curl "https://api.varshtat.com/v1/data/ohlcv/1m?feed=binance&sym=BTCUSDT&limit=5" \\
  -H "X-API-Key: $API_KEY"`}
          notes="Bars are ordered oldest to newest; start_ts_ms/end_ts_ms are minute boundaries in epoch ms (UTC). Feed timelines: kraken-spot is near-real-time; massive is ~15 minutes delayed; binance is near-real-time. Symbols: kraken-spot uses pairs like BTC/USDT (URL-encode as %2F); binance and massive use simple tickers (BTCUSDT, AAPL). v = base-currency volume (base asset, e.g. BTC for BTC/USD). quote_volume = quote-currency volume Σ(price×qty) (quote asset, e.g. USD for BTC/USD); 0.0 for sources without trade-level price/qty pairing (massive 1s OHLCV). trade_count = trades in the minute (0 for massive, which has no trade-level detail). taker_buy_volume/taker_sell_volume = aggressor volume split by side (0.0 if feed lacks aggressor data). market_order_volume = volume from market orders (populated only for kraken-spot via ord_type='market'; 0.0 for binance and massive). Use /v1/data/ohlcv/1m/symbols to list cached symbols. 404 if no bars cached for the symbol yet."
        />
        <Endpoint
          method="GET"
          path="/v1/data/ohlcv/1m/symbols"
          scope="datasets:read"
          description="List the symbols that currently have a 1-minute OHLCV ring cached, for a given feed."
          queryParams={[
            { name: "feed", description: "massive (US equities/ETFs), kraken-spot (crypto), or binance (crypto) (required)" },
          ]}
          response={`{
  "feed": "kraken-spot",
  "symbols": ["BTC/USDT", "ETH/USDT", "SOL/USDT", "XRP/USDT"],
  "count": 26,
  "served_at": "2026-06-27T02:27:13.836Z"
}`}
          curl={`curl "https://api.varshtat.com/v1/data/ohlcv/1m/symbols?feed=kraken-spot" \\
  -H "X-API-Key: $API_KEY"`}
          notes="Symbols are sorted ascending and capped at 500. Use one of these as the sym for /v1/data/ohlcv/1m (URL-encode any slash). A symbol appears here only once it has bars in the rolling ~60-minute cache."
        />
        <Endpoint
          method="GET"
          path="/v1/data/download"
          scope="datasets:read"
          description="Generate short-lived signed URLs for archived Parquet files on GCS. Use the returned signedUrl directly with curl, DuckDB, or pandas — no auth needed on the URL itself."
          queryParams={[
            { name: "feed", description: "kalshi, kraken-futures, kraken-spot, hols, or massive (required)" },
            { name: "from", description: "Start date, YYYY-MM-DD (required)" },
            { name: "to", description: "End date, YYYY-MM-DD — max 7-day range (required)" },
            { name: "type", description: "Filter by message type, e.g. trade, ticker, ohlcv (optional)" },
            { name: "expires", description: "Signed URL TTL, 1h–12h (default 12h), format like '6h' (optional)" },
          ]}
          response={`{
  "feed": "hols",
  "from": "2026-03-09",
  "to": "2026-03-09",
  "type": null,
  "files": [
    {
      "path": "hols/crypto/daily/2026-03-09/ohlcv-1m-ssmd.parquet",
      "name": "ohlcv-1m-ssmd.parquet",
      "type": "ohlcv-1m-ssmd",
      "hour": "2026-03-09",
      "bytes": 516889,
      "signedUrl": "https://storage.googleapis.com/ssmd-data/hols/...?X-Goog-Signature=...",
      "expiresAt": "2026-03-10T07:33:29.072Z"
    }
  ],
  "expiresIn": "12h"
}`}
          curl={`curl "https://api.varshtat.com/v1/data/download?feed=hols&from=2026-03-09&to=2026-03-09" \\
  -H "X-API-Key: $API_KEY"`}
          notes="Per-key feed and date-range restrictions are enforced (and clamped) server-side. Max 200 files per request — narrow the date range or filter by type if exceeded. hols crypto dataset includes: ohlcv-1m-ssmd.parquet (REST-sourced Kraken OHLCV) and ohlcv-1m-binance-ws.parquet (WS-sourced Binance 1m bars aggregated from trades). Both files carry base+quote volume, trade counts, and aggressor-side volume splits; binance WS has taker_buy_volume/taker_sell_volume derived from the is_buyer_maker flag (v1.1.0) and null marketorder_volume (no order-type data from Binance). See the DuckDB/Python examples below."
        />
        <Endpoint
          method="GET"
          path="/v1/data/schema-versions"
          scope="datasets:read"
          description="Current schema version per feed and message type (mirrors the Rust MessageSchema versions). Useful for detecting breaking parquet schema changes."
          response={`{
  "kalshi": {
    "ticker": { "version": "1.3.0", "notes": "cents integers or dollar strings (WS v2)" },
    "trade": { "version": "1.3.0", "notes": "cents integers or dollar strings (WS v2)" },
    "market_lifecycle_v2": { "version": "1.0.0" }
  },
  "kraken-spot": {
    "ticker": { "version": "1.0.0" },
    "trade": { "version": "1.0.0" }
  },
  "binance": {
    "trade": { "version": "1.1.0", "notes": "spot @trade; is_buyer_maker (taker side) materialized as of 1.1.0" }
  },
  "massive": {
    "trade": { "version": "1.0.0" },
    "quote": { "version": "1.0.0" },
    "ohlcv_1s": { "version": "1.0.0" },
    "ohlcv_1m": { "version": "1.0.0" }
  }
}`}
          curl={`curl "https://api.varshtat.com/v1/data/schema-versions" \\
  -H "X-API-Key: $API_KEY"`}
        />
        <Endpoint
          method="GET"
          path="/v1/data/whoami"
          scope="datasets:read"
          description="Resolved caller identity — email, scopes, and the feeds the key may access."
          response={`{
  "email": "user@example.com",
  "scopes": ["datasets:read"],
  "allowedFeeds": ["hols", "massive"]
}`}
          curl={`curl "https://api.varshtat.com/v1/data/whoami" \\
  -H "X-API-Key: $API_KEY"`}
        />
      </Section>

      {/* ===================================================================== */}
      {/* DOWNLOAD GUIDE */}
      {/* ===================================================================== */}
      <Section id="download-guide" title="Downloading Parquet — DuckDB &amp; Python">
        <div className="text-sm text-fg-muted space-y-3">
          <p>
            <code className="font-mono text-accent">/v1/data/download</code> returns signed GCS URLs.
            Read them directly (no download step) with DuckDB or pandas, or save the files with curl.
          </p>
        </div>
        <div className="border border-border rounded-lg p-4 bg-bg-raised space-y-3">
          <p className="text-sm font-semibold text-fg">curl — fetch URLs then download</p>
          <pre className="bg-bg font-mono text-xs text-fg-muted p-3 rounded border border-border overflow-x-auto whitespace-pre-wrap">{`# Get signed URLs, then download every file in the response
curl -s -H "X-API-Key: $API_KEY" \\
  'https://api.varshtat.com/v1/data/download?feed=hols&from=2026-03-09&to=2026-03-09' \\
  | jq -r '.files[] | "\\(.name) \\(.signedUrl)"' \\
  | while read name url; do curl -s -o "$name" "$url"; echo "Downloaded $name"; done`}</pre>
          <p className="text-sm font-semibold text-fg">DuckDB — query parquet over HTTP</p>
          <pre className="bg-bg font-mono text-xs text-fg-muted p-3 rounded border border-border overflow-x-auto whitespace-pre-wrap">{`-- Paste a signedUrl from the API response; DuckDB reads parquet over HTTPS
SELECT hols_ticker, count(*) AS bars, min(date) AS first, max(date) AS last
FROM read_parquet('https://storage.googleapis.com/ssmd-data/hols/...?X-Goog-Signature=...')
GROUP BY hols_ticker
ORDER BY bars DESC;`}</pre>
          <p className="text-sm font-semibold text-fg">Python — pandas + pyarrow</p>
          <pre className="bg-bg font-mono text-xs text-fg-muted p-3 rounded border border-border overflow-x-auto whitespace-pre-wrap">{`import requests, pandas as pd

API_KEY = "<your-api-key>"
resp = requests.get(
    "https://api.varshtat.com/v1/data/download",
    headers={"X-API-Key": API_KEY},
    params={"feed": "hols", "from": "2026-03-09", "to": "2026-03-09"},
)
files = resp.json()["files"]
df = pd.concat([pd.read_parquet(f["signedUrl"]) for f in files], ignore_index=True)
print(f"Loaded {len(df)} rows from {len(files)} files")`}</pre>
        </div>
      </Section>

      {/* ===================================================================== */}
      {/* MONITOR */}
      {/* ===================================================================== */}
      <Section id="monitor" title="Monitor (Live Browse)">
        <div className="text-sm text-fg-muted">
          Hierarchical market browser backed by Redis, with live prices merged in. Drill down
          categories → series → events → markets.
        </div>
        <Endpoint
          method="GET"
          path="/v1/monitor/categories"
          scope="datasets:read"
          description="List market categories (top of the hierarchy)."
          response={`{ "categories": [ { "name": "Crypto", "event_count": 12, "series_count": 3 } ] }`}
          curl={`curl "https://api.varshtat.com/v1/monitor/categories" \\
  -H "X-API-Key: $API_KEY"`}
        />
        <Endpoint
          method="GET"
          path="/v1/monitor/series"
          scope="datasets:read"
          description="List series within a category."
          queryParams={[{ name: "category", description: "Category name (required)" }]}
          response={`{ "series": [ { "ticker": "KXBTCD", "title": "Bitcoin Daily", "event_count": 30, "market_count": 150 } ] }`}
          curl={`curl "https://api.varshtat.com/v1/monitor/series?category=Crypto" \\
  -H "X-API-Key: $API_KEY"`}
        />
        <Endpoint
          method="GET"
          path="/v1/monitor/events"
          scope="datasets:read"
          description="List events within a series."
          queryParams={[{ name: "series", description: "Series ticker (required)" }]}
          response={`{ "events": [ { "ticker": "KXBTCD-26JUN2611", "title": "BTC Jun 26 11AM", "status": "active", "market_count": 24 } ] }`}
          curl={`curl "https://api.varshtat.com/v1/monitor/events?series=KXBTCD" \\
  -H "X-API-Key: $API_KEY"`}
        />
        <Endpoint
          method="GET"
          path="/v1/monitor/markets"
          scope="datasets:read"
          description="List markets within an event, with live snapshot prices merged in."
          queryParams={[{ name: "event", description: "Event ticker (required)" }]}
          response={`{
  "markets": [
    {
      "ticker": "KXBTCD-26JUN2611-B105000",
      "title": "Bitcoin above $105,000",
      "status": "active",
      "yes_bid": 0.45, "yes_ask": 0.47, "last": 0.46,
      "volume": 12030, "open_interest": 4521,
      "snap_at": "2026-06-26T10:00:00Z"
    }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/monitor/markets?event=KXBTCD-26JUN2611" \\
  -H "X-API-Key: $API_KEY"`}
        />
        <Endpoint
          method="GET"
          path="/v1/monitor/search"
          scope="datasets:read"
          description="Search events/series and outcomes directly from the DB, with live prices."
          queryParams={[
            { name: "q", description: "Search query (required)" },
            { name: "exchange", description: "kalshi, kraken, or polymarket" },
            { name: "type", description: "events and/or outcomes (default both)" },
            { name: "limit", description: "Max results (default 50, max 200)" },
          ]}
          response={`{
  "query": "bitcoin",
  "count": 2,
  "results": [
    { "ticker": "KXBTCD", "title": "Bitcoin Daily", "exchange": "kalshi", "type": "event", "status": "active" }
  ]
}`}
          curl={`curl "https://api.varshtat.com/v1/monitor/search?q=bitcoin&limit=20" \\
  -H "X-API-Key: $API_KEY"`}
        />
      </Section>

      {/* ===================================================================== */}
      {/* MCP */}
      {/* ===================================================================== */}
      <Section id="mcp" title="MCP Setup">
        <div className="text-sm text-fg-muted space-y-3">
          <p>
            The ssmd MCP server exposes the data, monitor, secmaster, and harman tools to any
            MCP-compatible client (Claude Desktop, Claude Code, Cursor). Requires an API key with the
            relevant scopes and{" "}
            <a
              href="https://docs.astral.sh/uv/"
              target="_blank"
              rel="noopener noreferrer"
              className="text-accent hover:underline"
            >
              uv
            </a>
            .
          </p>
        </div>
        <div className="border border-border rounded-lg p-4 bg-bg-raised space-y-3">
          <p className="text-sm font-semibold text-fg">Install</p>
          <pre className="bg-bg font-mono text-xs text-fg-muted p-3 rounded border border-border overflow-x-auto whitespace-pre-wrap">{`git clone https://github.com/aaronwald/ssmd.git
cd ssmd/ssmd-mcp
uv sync`}</pre>
          <p className="text-sm font-semibold text-fg">Add to your MCP client (.mcp.json)</p>
          <pre className="bg-bg font-mono text-xs text-fg-muted p-3 rounded border border-border overflow-x-auto whitespace-pre-wrap">{`{
  "mcpServers": {
    "ssmd": {
      "command": "uv",
      "args": ["--directory", "/path/to/ssmd/ssmd-mcp", "run", "ssmd-mcp"],
      "env": {
        "SSMD_API_URL": "https://api.varshtat.com",
        "SSMD_API_KEY": "<your-api-key>"
      }
    }
  }
}`}</pre>
          <p className="text-xs text-fg-muted">
            Tools: data (query_trades, query_prices, query_snap, query_events, query_volume,
            lookup_market, list_feeds, check_freshness), monitor (browse_categories/series/events/markets),
            secmaster (secmaster_stats, search_markets/events/pairs/conditions/lifecycle, get_fees),
            and harman OMS (sessions, orders, fills, timeline, audit, settlements — admin scope).
          </p>
          <p className="text-xs text-fg-muted">
            Schema reference:{" "}
            <a
              href="https://github.com/aaronwald/ssmd/blob/main/docs/schemas/parquet-schemas.md"
              target="_blank"
              rel="noopener noreferrer"
              className="text-accent hover:underline"
            >
              Parquet schemas
            </a>
            ,{" "}
            <a
              href="https://github.com/aaronwald/ssmd/blob/main/docs/schemas/kalshi-json.md"
              target="_blank"
              rel="noopener noreferrer"
              className="text-accent hover:underline"
            >
              Kalshi JSON
            </a>
            ,{" "}
            <a
              href="https://github.com/aaronwald/ssmd/blob/main/docs/schemas/kraken-futures-json.md"
              target="_blank"
              rel="noopener noreferrer"
              className="text-accent hover:underline"
            >
              Kraken Futures JSON
            </a>
            ,{" "}
            <a
              href="https://github.com/aaronwald/ssmd/blob/main/docs/schemas/polymarket-json.md"
              target="_blank"
              rel="noopener noreferrer"
              className="text-accent hover:underline"
            >
              Polymarket JSON
            </a>
            .
          </p>
        </div>
      </Section>
    </>
  );
}
