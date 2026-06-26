import { Section, TypeTable } from "./_components";

// ===========================================================================
// Feeds & Protocols
//
// Documents each upstream market-data FEED, the MESSAGE TYPES it carries, and a
// compact key-field summary per message type, with deep links to the full
// schema docs in the public ssmd repo.
//
// Field summaries are sourced from the real schema docs / generator code:
//   - Kalshi:           docs/schemas/kalshi-json.md + docs/schemas/parquet-schemas.md
//   - Kraken Spot:      docs/schemas/parquet-schemas.md (Kraken Spot Schemas)
//   - Kraken Futures:   docs/schemas/kraken-futures-json.md + parquet-schemas.md
//   - Massive (Polygon): ssmd-agent hols-massive.ts / routes.ts OHLCV bar shape
//   - HOLS:             ssmd-agent hols.ts NdjsonRow / daily aggregate columns
//   - Polymarket:       docs/schemas/polymarket-json.md + parquet-schemas.md
// Schema versions come from ssmd-agent/src/server/schema-versions.json and the
// parquet-schemas.md versioning table. Feed list + message types mirror
// FEED_CONFIG in ssmd-agent/src/lib/gcs/signed-urls.ts.
// ===========================================================================

const SCHEMA_DOC_BASE = "https://github.com/aaronwald/ssmd/blob/main/docs/schemas";

interface MessageType {
  /** Message type as it appears in FEED_CONFIG (e.g. "ticker", "ohlcv_1m"). */
  name: string;
  /** Schema version, when one is tracked for this message type. */
  version?: string;
  /** Optional one-line note shown under the field table. */
  note?: string;
  /** Representative key fields (a summary, not the exhaustive list). */
  fields: { value: string; description: string }[];
}

interface FeedBlockProps {
  /** Feed name as used by the data API / FEED_CONFIG. */
  name: string;
  /** Transport / protocol summary. */
  transport: string;
  /** What the feed carries. */
  description: string;
  /** Optional status callout (e.g. decommissioned / archived-only feeds). */
  status?: string;
  messageTypes: MessageType[];
  /** Deep link to the full schema doc. */
  docHref: string;
  docLabel: string;
}

function FeedBlock({
  name,
  transport,
  description,
  status,
  messageTypes,
  docHref,
  docLabel,
}: FeedBlockProps) {
  return (
    <div
      id={`feed-${name}`}
      className="border border-border rounded-lg p-4 bg-bg space-y-3 scroll-mt-20"
    >
      <div>
        <div className="flex items-center gap-3 flex-wrap">
          <h3 className="text-base font-bold text-fg font-mono">{name}</h3>
          <span className="text-xs text-fg-subtle">{transport}</span>
        </div>
        <p className="text-sm text-fg-muted mt-2">{description}</p>
        {status && <p className="text-xs text-amber-400 mt-1">{status}</p>}
      </div>

      {messageTypes.map((mt) => (
        <div key={mt.name} className="space-y-1">
          <TypeTable
            name={`${mt.name}${mt.version ? `  ·  schema v${mt.version}` : ""}`}
            values={mt.fields}
          />
          {mt.note && <p className="text-xs text-fg-subtle">{mt.note}</p>}
        </div>
      ))}

      <p className="text-xs text-fg-muted">
        Full field reference:{" "}
        <a
          href={docHref}
          target="_blank"
          rel="noopener noreferrer"
          className="text-accent hover:underline"
        >
          {docLabel}
        </a>
      </p>
    </div>
  );
}

export function FeedsProtocolsSections() {
  return (
    <Section id="feeds-protocols" title="Feeds & Protocols">
      <div className="text-sm text-fg-muted space-y-3">
        <p>
          A <span className="text-fg font-semibold">feed</span> is a single upstream
          market-data source — an exchange (or aggregator) reached over a specific transport,
          captured continuously and archived to GCS as Parquet. A{" "}
          <span className="text-fg font-semibold">message type</span> is a distinct record shape
          within a feed: for example <code className="font-mono text-accent">ticker</code> (quote
          snapshots) and <code className="font-mono text-accent">trade</code> (executions) are two
          message types of the same feed, each with its own columns and schema version.
        </p>
        <p>
          The tables below give a <span className="text-fg font-semibold">compact, representative</span>{" "}
          subset of each message type&apos;s key fields — enough to know what a feed carries. Every
          message type also includes the pipeline-injected columns{" "}
          <code className="font-mono text-accent">_nats_seq</code> and{" "}
          <code className="font-mono text-accent">_received_at</code>. The exhaustive,
          column-by-column field tables (types, nullability, JSON source paths) live in the linked
          schema docs.
        </p>
      </div>

      {/* ===================================================================== */}
      {/* KALSHI */}
      {/* ===================================================================== */}
      <FeedBlock
        name="kalshi"
        transport="WebSocket — wss://api.kalshi.com/trade-api/ws/v2"
        description="Kalshi crypto markets (incl. the 15-minute KX…15M contracts). Envelope-wrapped JSON ({ type, sid, msg }); prices are cents (0–99), timestamps are Unix seconds."
        messageTypes={[
          {
            name: "ticker",
            version: "1.3.0",
            fields: [
              { value: "market_ticker: Utf8", description: "Market identifier, e.g. KXBTCD-26JUN2611-T105000" },
              { value: "yes_bid / yes_ask: Int64", description: "Best YES bid/ask in cents (0–99)" },
              { value: "no_bid / no_ask: Int64", description: "Best NO bid/ask in cents (0–99)" },
              { value: "last_price: Int64", description: "Last trade price in cents (from msg.price)" },
              { value: "volume / open_interest: Int64", description: "Contracts traded / current open interest" },
              { value: "ts: Timestamp(us, UTC)", description: "Exchange timestamp (Unix seconds → micros)" },
              { value: "exchange_clock: Int64", description: "Kalshi global monotonic clock (msg.Clock)" },
            ],
          },
          {
            name: "trade",
            version: "1.3.0",
            note: "Has a per-subscription exchange_seq (nullable) used for gap detection.",
            fields: [
              { value: "market_ticker: Utf8", description: "Market identifier" },
              { value: "price: Int64", description: "YES-side trade price in cents (msg.yes_price / msg.price)" },
              { value: "count: Int64", description: "Number of contracts traded" },
              { value: "side: Utf8", description: "Taker side — \"yes\" or \"no\" (msg.taker_side / msg.side)" },
              { value: "trade_id: Utf8", description: "Unique trade UUID" },
              { value: "ts: Timestamp(us, UTC)", description: "Trade execution timestamp" },
              { value: "exchange_seq: Int64", description: "Per-subscription sequence (envelope seq)" },
            ],
          },
          {
            name: "market_lifecycle_v2",
            version: "1.0.0",
            fields: [
              { value: "market_ticker: Utf8", description: "Market identifier" },
              { value: "event_type: Utf8", description: "created, activated, deactivated, determined, settled…" },
              { value: "open_ts: Timestamp(us, UTC)", description: "When the market opens for trading (nullable)" },
              { value: "close_ts: Timestamp(us, UTC)", description: "When the market closes (nullable)" },
              { value: "additional_metadata: Utf8", description: "Extra context as a serialized JSON string (nullable)" },
            ],
          },
        ]}
        docHref={`${SCHEMA_DOC_BASE}/kalshi-json.md`}
        docLabel="kalshi-json.md"
      />

      {/* ===================================================================== */}
      {/* KRAKEN SPOT */}
      {/* ===================================================================== */}
      <FeedBlock
        name="kraken-spot"
        transport="WebSocket — wss://ws.kraken.com/v2"
        description="Kraken spot markets. V2 protocol: { channel, type, data:[…] } where each data item becomes a row. Prices/sizes are decimals; trade timestamps are ISO-8601."
        messageTypes={[
          {
            name: "ticker",
            version: "1.0.0",
            fields: [
              { value: "symbol: Utf8", description: "Pair, e.g. BTC/USD" },
              { value: "bid / ask: Float64", description: "Best bid / ask price" },
              { value: "bid_qty / ask_qty: Float64", description: "Size at best bid / ask" },
              { value: "last: Float64", description: "Last traded price" },
              { value: "volume: Float64", description: "24h volume" },
              { value: "vwap / high / low: Float64", description: "24h VWAP, high, low" },
            ],
          },
          {
            name: "trade",
            version: "1.0.0",
            fields: [
              { value: "symbol: Utf8", description: "Pair, e.g. BTC/USD" },
              { value: "side: Utf8", description: "\"buy\" or \"sell\"" },
              { value: "price: Float64", description: "Trade price" },
              { value: "qty: Float64", description: "Trade quantity" },
              { value: "ord_type: Utf8", description: "market, limit, …" },
              { value: "trade_id: Utf8", description: "Trade identifier (coerced to string)" },
              { value: "timestamp: Timestamp(us, UTC)", description: "Trade time (ISO-8601 → micros)" },
            ],
          },
        ]}
        docHref={`${SCHEMA_DOC_BASE}/parquet-schemas.md`}
        docLabel="parquet-schemas.md (Kraken Spot)"
      />

      {/* ===================================================================== */}
      {/* KRAKEN FUTURES */}
      {/* ===================================================================== */}
      <FeedBlock
        name="kraken-futures"
        transport="WebSocket — wss://futures.kraken.com/ws/v1"
        description="Kraken perpetual/futures markets. V1 protocol: flat JSON with feed + product_id at top level; mixed camelCase/snake_case fields; timestamps are epoch milliseconds."
        status="Connector dead since 2026-06-03 — archived/historical Parquet data only, no live capture."
        messageTypes={[
          {
            name: "ticker",
            version: "1.0.0",
            fields: [
              { value: "product_id: Utf8", description: "Product symbol, e.g. PF_XBTUSD, PI_ETHUSD" },
              { value: "bid / ask: Float64", description: "Best bid / ask price" },
              { value: "last: Float64", description: "Last traded price" },
              { value: "volume: Float64", description: "24h volume (base currency)" },
              { value: "mark_price: Float64", description: "Mark price (from markPrice)" },
              { value: "index_price: Float64", description: "Underlying index price (from index)" },
              { value: "funding_rate: Float64", description: "Current hourly funding rate" },
              { value: "open_interest: Float64", description: "Open interest (from openInterest)" },
              { value: "time: Timestamp(us, UTC)", description: "Message time (epoch ms → micros)" },
            ],
          },
          {
            name: "trade",
            version: "1.0.0",
            note: "seq is the per-product exchange sequence, used for gap detection.",
            fields: [
              { value: "product_id: Utf8", description: "Product symbol" },
              { value: "uid: Utf8", description: "Trade UUID" },
              { value: "side: Utf8", description: "Taker side — \"buy\" or \"sell\"" },
              { value: "trade_type: Utf8", description: "Trade type, e.g. fill (from JSON type)" },
              { value: "seq: Int64", description: "Per-product exchange sequence number" },
              { value: "qty / price: Float64", description: "Trade quantity / price" },
              { value: "time: Timestamp(us, UTC)", description: "Trade time (epoch ms → micros)" },
            ],
          },
        ]}
        docHref={`${SCHEMA_DOC_BASE}/kraken-futures-json.md`}
        docLabel="kraken-futures-json.md"
      />

      {/* ===================================================================== */}
      {/* MASSIVE (POLYGON.IO) */}
      {/* ===================================================================== */}
      <FeedBlock
        name="massive"
        transport="Polygon.io aggregates (AM bars over WebSocket)"
        description="US-equities OHLCV bars from Polygon.io. Raw 1-second and 1-minute bars are archived directly; the daily (1d) bar is a derived aggregate (one row per symbol/day). The live feed is ~15 minutes delayed."
        messageTypes={[
          {
            name: "ohlcv_1s / ohlcv_1m",
            version: "1.0.0",
            note: "Polygon emits multiple cumulative snapshots per minute; the final bar per (symbol, start_ts_ms) is the canonical one.",
            fields: [
              { value: "symbol: Utf8", description: "Equity/ETF symbol, e.g. AAPL, SPY" },
              { value: "open / high / low / close: Float64", description: "Bar OHLC prices" },
              { value: "volume: Float64", description: "Bar volume (whole-share)" },
              { value: "vwap: Float64", description: "Volume-weighted average price" },
              { value: "start_ts_ms / end_ts_ms: Int64", description: "Bar boundaries in epoch milliseconds (UTC)" },
            ],
          },
          {
            name: "ohlcv_1d",
            version: "1.0.0",
            note: "Daily aggregate written by `hols aggregate --source massive` (flat layout).",
            fields: [
              { value: "symbol: Utf8", description: "Equity/ETF symbol" },
              { value: "date: Date", description: "Trading day (UTC)" },
              { value: "open / high / low / close: Float64", description: "Daily OHLC" },
              { value: "volume: Float64", description: "Daily volume (sum of final 1m bars)" },
              { value: "vwap: Float64", description: "Volume-weighted daily VWAP" },
              { value: "bar_count: Int64", description: "Number of 1m bars aggregated" },
              { value: "first_bar_ts_ms / last_bar_ts_ms: Int64", description: "First/last contributing bar (epoch ms)" },
            ],
          },
        ]}
        docHref={`${SCHEMA_DOC_BASE}/parquet-schemas.md`}
        docLabel="parquet-schemas.md"
      />

      {/* ===================================================================== */}
      {/* HOLS */}
      {/* ===================================================================== */}
      <FeedBlock
        name="hols"
        transport="Daily aggregation job (Binance & Kraken sources)"
        description="Crypto OHLCV candles — 1-minute & 5-minute bars (open/high/low/close/volume) derived daily from Binance and Kraken into one Parquet file per day. Powers the v14 model inputs."
        messageTypes={[
          {
            name: "ohlcv",
            note: "Derived daily dataset (no exchange schema version). source identifies the upstream, e.g. kraken_spot_trades or binance_spot.",
            fields: [
              { value: "hols_ticker: Utf8", description: "Normalized ticker, e.g. BTCUSDT" },
              { value: "symbol: Utf8", description: "Upstream pair symbol" },
              { value: "source: Utf8", description: "Origin of the bar, e.g. binance_spot, kraken_spot, kraken_spot_trades" },
              { value: "date / date_close: Timestamp", description: "Bar open / close minute boundary" },
              { value: "open / high / low / close: Double", description: "Candle OHLC" },
              { value: "volume: Double", description: "Base-asset volume" },
              { value: "volume_from: Double", description: "Quote-asset volume (nullable)" },
              { value: "tradecount: Int64", description: "Number of trades in the bar (nullable)" },
            ],
          },
        ]}
        docHref={`${SCHEMA_DOC_BASE}/parquet-schemas.md`}
        docLabel="parquet-schemas.md"
      />

      {/* ===================================================================== */}
      {/* POLYMARKET */}
      {/* ===================================================================== */}
      <FeedBlock
        name="polymarket"
        transport="CLOB WebSocket — wss://ws-subscriptions-clob.polymarket.com/ws/market"
        description="Polymarket prediction markets. Flat JSON keyed by event_type; identifiers are asset_id (token id) and market (condition id, 0x hex). Numeric values are decimal strings to preserve precision."
        status="Decommissioned — historical GCS/Parquet data only, no live capture."
        messageTypes={[
          {
            name: "book",
            version: "1.0.0",
            note: "Order book levels are stored as serialized JSON arrays of { price, size }.",
            fields: [
              { value: "asset_id: Utf8", description: "Outcome token id (YES or NO)" },
              { value: "market: Utf8", description: "Condition id (0x-prefixed hex)" },
              { value: "timestamp_ms: Int64", description: "Snapshot time, Unix milliseconds" },
              { value: "hash: Utf8", description: "Order-book summary hash (nullable)" },
              { value: "bids_json / asks_json: Utf8", description: "JSON-serialized level arrays (buys/sells)" },
            ],
          },
          {
            name: "last_trade_price",
            version: "2.1.0",
            fields: [
              { value: "asset_id: Utf8", description: "Token that traded" },
              { value: "market: Utf8", description: "Condition id" },
              { value: "price: Utf8", description: "Trade price as decimal string (0.00–1.00)" },
              { value: "side: Utf8", description: "Taker side — \"BUY\" or \"SELL\" (nullable)" },
              { value: "size: Utf8", description: "Trade size as string" },
              { value: "fee_rate_bps: Utf8", description: "Fee rate in basis points (nullable)" },
              { value: "timestamp_ms: Int64", description: "Unix milliseconds" },
            ],
          },
          {
            name: "price_change",
            version: "2.0.0",
            note: "Fans out: one message's price_changes[] array becomes N rows.",
            fields: [
              { value: "market: Utf8", description: "Condition id" },
              { value: "asset_id: Utf8", description: "Token affected by the change" },
              { value: "price / size: Utf8", description: "Level price / new aggregate size (\"0\" = removed)" },
              { value: "side: Utf8", description: "\"BUY\" or \"SELL\"" },
              { value: "best_bid / best_ask: Utf8", description: "Updated top of book (nullable)" },
              { value: "timestamp_ms: Int64", description: "Unix milliseconds" },
            ],
          },
          {
            name: "best_bid_ask",
            version: "2.0.0",
            fields: [
              { value: "market: Utf8", description: "Condition id" },
              { value: "asset_id: Utf8", description: "Token id" },
              { value: "best_bid / best_ask: Utf8", description: "Top-of-book quote (nullable)" },
              { value: "spread: Utf8", description: "Bid-ask spread (nullable)" },
              { value: "timestamp_ms: Int64", description: "Unix milliseconds" },
            ],
          },
        ]}
        docHref={`${SCHEMA_DOC_BASE}/polymarket-json.md`}
        docLabel="polymarket-json.md"
      />
    </Section>
  );
}
