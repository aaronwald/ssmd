import { Endpoint, Section, TypeTable } from "./_components";
import { DataApiSections } from "./_data-api";
import { FeedsProtocolsSections } from "./_feeds";

// ---------------------------------------------------------------------------
// Shared UI primitives (Endpoint, Section, TypeTable, etc.) live in
// ./_components.tsx so this page and ./_data-api.tsx render identically.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// TOC
// ---------------------------------------------------------------------------

const tocGroups = [
  {
    label: "Harman OMS API",
    sections: [
      { id: "authentication", label: "Authentication" },
      { id: "orders", label: "Orders" },
      { id: "order-groups", label: "Order Groups" },
      { id: "fills-audit", label: "Fills & Audit" },
      { id: "oms-market-data", label: "Market Data" },
      { id: "account", label: "Account" },
      { id: "admin", label: "Admin" },
      { id: "types", label: "Types" },
    ],
  },
  {
    label: "Market Data API",
    sections: [
      { id: "market-data-api", label: "Overview" },
      { id: "secmaster", label: "Secmaster" },
      { id: "kraken-pairs", label: "Kraken Pairs" },
      { id: "fees", label: "Fees" },
      { id: "data-endpoints", label: "Data" },
      { id: "download-guide", label: "Download Guide" },
      { id: "monitor", label: "Monitor" },
      { id: "mcp", label: "MCP Setup" },
    ],
  },
  {
    label: "Feeds & Protocols",
    sections: [
      { id: "feeds-protocols", label: "Feeds & Protocols" },
    ],
  },
];

const tocSectionsFlat = tocGroups.flatMap((g) => g.sections);

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

export default function DocsPage() {
  return (
    <div className="flex gap-8 max-w-7xl mx-auto px-4 py-8">
      {/* TOC sidebar (desktop) */}
      <nav className="hidden lg:block w-48 shrink-0 sticky top-20 self-start">
        <p className="text-xs font-semibold text-fg-muted uppercase tracking-wider mb-3">
          Contents
        </p>
        <div className="space-y-4">
          {tocGroups.map((g) => (
            <div key={g.label}>
              <p className="text-[11px] font-semibold text-fg-subtle uppercase tracking-wider mb-1.5">
                {g.label}
              </p>
              <ul className="space-y-1.5">
                {g.sections.map((s) => (
                  <li key={s.id}>
                    <a
                      href={`#${s.id}`}
                      className="text-sm text-fg-muted hover:text-accent transition-colors"
                    >
                      {s.label}
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      </nav>

      <main className="min-w-0 flex-1">
        <h1 className="text-2xl font-bold text-fg mb-1">API Reference</h1>
        <p className="text-sm text-fg-muted mb-6">
          Two APIs are documented here. The <strong className="text-fg">Harman OMS API</strong>{" "}
          (base URL depends on deployment, bearer-token auth) covers order management. The{" "}
          <strong className="text-fg">Market Data API</strong> at{" "}
          <code className="font-mono text-accent">https://api.varshtat.com</code> (X-API-Key auth)
          covers market metadata and the data archive. OMS prices and quantities are decimal strings
          (e.g. <code className="font-mono text-accent">&quot;0.42&quot;</code> = 42 cents).
        </p>

        {/* Mobile TOC */}
        <div className="lg:hidden mb-6 flex flex-wrap gap-2">
          {tocSectionsFlat.map((s) => (
            <a
              key={s.id}
              href={`#${s.id}`}
              className="text-xs px-2 py-1 rounded bg-bg-surface text-fg-muted hover:text-accent transition-colors"
            >
              {s.label}
            </a>
          ))}
        </div>

        {/* ============================================================= */}
        {/* AUTHENTICATION */}
        {/* ============================================================= */}
        <Section id="authentication" title="Authentication">
          <div className="text-sm text-fg-muted space-y-3">
            <p>
              The Harman OMS API supports four authentication methods. Include credentials on every
              request. (The Market Data API below uses a separate X-API-Key — see its overview.)
            </p>
            <div className="space-y-4">
              <div className="border border-border rounded p-3 bg-bg">
                <p className="font-semibold text-fg text-sm">1. Cloudflare JWT (browser sessions)</p>
                <p className="text-xs text-fg-muted mt-1">
                  Automatically set by Cloudflare Access as a cookie (<code className="font-mono text-accent">CF_Authorization</code>).
                  Used by the web UI. Scopes derived from the CF email&apos;s API key entry.
                </p>
              </div>
              <div className="border border-border rounded p-3 bg-bg">
                <p className="font-semibold text-fg text-sm">2. API Key (programmatic access)</p>
                <p className="text-xs text-fg-muted mt-1">
                  Pass as <code className="font-mono text-accent">Authorization: Bearer &lt;key&gt;</code> header
                  or <code className="font-mono text-accent">x-api-key: &lt;key&gt;</code> header. Scopes and rate limits set at key creation.
                </p>
              </div>
              <div className="border border-border rounded p-3 bg-bg">
                <p className="font-semibold text-fg text-sm">3. Static Token</p>
                <p className="text-xs text-fg-muted mt-1">
                  Server-configured bearer token. Grants <code className="font-mono text-accent">harman:read</code> + <code className="font-mono text-accent">harman:write</code> scopes.
                </p>
              </div>
              <div className="border border-border rounded p-3 bg-bg">
                <p className="font-semibold text-fg text-sm">4. Static Admin Token</p>
                <p className="text-xs text-fg-muted mt-1">
                  Server-configured bearer token with full <code className="font-mono text-accent">harman:admin</code> scope (includes read + write).
                </p>
              </div>
            </div>
            <div className="border border-border rounded p-3 bg-bg mt-4">
              <p className="font-semibold text-fg text-sm">Scope Hierarchy</p>
              <pre className="font-mono text-xs text-fg-muted mt-2">harman:admin &gt; harman:write &gt; harman:read</pre>
              <p className="text-xs text-fg-muted mt-1">
                Admin scope implies write + read. Write scope implies read.
              </p>
            </div>
          </div>
        </Section>

        {/* ============================================================= */}
        {/* ORDERS */}
        {/* ============================================================= */}
        <Section id="orders" title="Orders">
          <Endpoint
            method="POST"
            path="/v1/orders"
            scope="harman:write"
            description="Create a new order."
            body={`{
  "client_order_id": "my-order-001",
  "ticker": "KXBTCD-26MAR28-B50000",
  "side": "yes",
  "action": "buy",
  "quantity": "10",
  "price_dollars": "0.42",
  "time_in_force": "gtc"
}`}
            response={`{
  "id": "ord_abc123",
  "client_order_id": "my-order-001",
  "status": "pending"
}`}
            curl={`curl -X POST $HARMAN_URL/v1/orders \\
  -H "Authorization: Bearer $HARMAN_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{"client_order_id":"my-order-001","ticker":"KXBTCD-26MAR28-B50000","side":"yes","action":"buy","quantity":"10","price_dollars":"0.42"}'`}
            notes="Idempotency: duplicate client_order_id returns 409, or 200 with x-idempotent-replay: true header if the order already progressed."
          />
          <Endpoint
            method="GET"
            path="/v1/orders"
            scope="harman:read"
            description="List orders for the current session."
            queryParams={[
              { name: "state", description: "Filter by state group (open, terminal, resting, today) or individual state" },
            ]}
            response={`{
  "orders": [
    {
      "id": "ord_abc123",
      "client_order_id": "my-order-001",
      "ticker": "KXBTCD-26MAR28-B50000",
      "side": "yes",
      "action": "buy",
      "quantity": "10",
      "filled_quantity": "0",
      "price_dollars": "0.42",
      "state": "resting",
      "time_in_force": "gtc",
      "created_at": "2026-03-01T12:00:00Z",
      "updated_at": "2026-03-01T12:00:01Z"
    }
  ]
}`}
            curl={`curl $HARMAN_URL/v1/orders?state=open \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/v1/orders/:id"
            scope="harman:read"
            description="Get a single order by ID."
            response={`{
  "id": "ord_abc123",
  "client_order_id": "my-order-001",
  "ticker": "KXBTCD-26MAR28-B50000",
  "side": "yes",
  "action": "buy",
  "quantity": "10",
  "filled_quantity": "5",
  "price_dollars": "0.42",
  "state": "resting",
  "time_in_force": "gtc",
  "created_at": "2026-03-01T12:00:00Z",
  "updated_at": "2026-03-01T12:00:05Z"
}`}
            curl={`curl $HARMAN_URL/v1/orders/ord_abc123 \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="DELETE"
            path="/v1/orders/:id"
            scope="harman:write"
            description="Cancel an open order."
            response={`{ "status": "pending_cancel" }`}
            curl={`curl -X DELETE $HARMAN_URL/v1/orders/ord_abc123 \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="POST"
            path="/v1/orders/:id/amend"
            scope="harman:write"
            description="Amend an open order's price and/or quantity."
            body={`{
  "new_price_dollars": "0.45",
  "new_quantity": "15"
}`}
            response={`{ "status": "pending_amend" }`}
            curl={`curl -X POST $HARMAN_URL/v1/orders/ord_abc123/amend \\
  -H "Authorization: Bearer $HARMAN_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{"new_price_dollars":"0.45"}'`}
            notes="At least one of new_price_dollars or new_quantity must be provided."
          />
          <Endpoint
            method="POST"
            path="/v1/orders/:id/decrease"
            scope="harman:write"
            description="Decrease the quantity of an open order."
            body={`{
  "reduce_by": "5"
}`}
            response={`{ "status": "pending_decrease" }`}
            curl={`curl -X POST $HARMAN_URL/v1/orders/ord_abc123/decrease \\
  -H "Authorization: Bearer $HARMAN_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{"reduce_by":"5"}'`}
          />
        </Section>

        {/* ============================================================= */}
        {/* ORDER GROUPS */}
        {/* ============================================================= */}
        <Section id="order-groups" title="Order Groups">
          <Endpoint
            method="POST"
            path="/v1/groups/bracket"
            scope="harman:write"
            description="Create a bracket order group: entry + take-profit + stop-loss."
            body={`{
  "entry": {
    "client_order_id": "brk-entry",
    "ticker": "KXBTCD-26MAR28-B50000",
    "side": "yes",
    "action": "buy",
    "quantity": "10",
    "price_dollars": "0.42"
  },
  "take_profit": {
    "client_order_id": "brk-tp",
    "ticker": "KXBTCD-26MAR28-B50000",
    "side": "yes",
    "action": "sell",
    "quantity": "10",
    "price_dollars": "0.65"
  },
  "stop_loss": {
    "client_order_id": "brk-sl",
    "ticker": "KXBTCD-26MAR28-B50000",
    "side": "yes",
    "action": "sell",
    "quantity": "10",
    "price_dollars": "0.30"
  }
}`}
            response={`{
  "group_id": "grp_abc123",
  "orders": [
    { "id": "ord_1", "client_order_id": "brk-entry", "leg_role": "entry", "status": "pending" },
    { "id": "ord_2", "client_order_id": "brk-tp", "leg_role": "take_profit", "status": "pending" },
    { "id": "ord_3", "client_order_id": "brk-sl", "leg_role": "stop_loss", "status": "pending" }
  ]
}`}
            curl={`curl -X POST $HARMAN_URL/v1/groups/bracket \\
  -H "Authorization: Bearer $HARMAN_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{"entry":{"client_order_id":"brk-entry","ticker":"KXBTCD-26MAR28-B50000","side":"yes","action":"buy","quantity":"10","price_dollars":"0.42"},"take_profit":{"client_order_id":"brk-tp","ticker":"KXBTCD-26MAR28-B50000","side":"yes","action":"sell","quantity":"10","price_dollars":"0.65"},"stop_loss":{"client_order_id":"brk-sl","ticker":"KXBTCD-26MAR28-B50000","side":"yes","action":"sell","quantity":"10","price_dollars":"0.30"}}'`}
            notes="Entry is placed immediately. TP and SL are held until entry fills, then both are placed. When one exit fills, the other is cancelled."
          />
          <Endpoint
            method="POST"
            path="/v1/groups/oco"
            scope="harman:write"
            description="Create a one-cancels-other order group: two legs, first fill cancels the other."
            body={`{
  "leg1": {
    "client_order_id": "oco-leg1",
    "ticker": "KXBTCD-26MAR28-B50000",
    "side": "yes",
    "action": "buy",
    "quantity": "10",
    "price_dollars": "0.40"
  },
  "leg2": {
    "client_order_id": "oco-leg2",
    "ticker": "KXBTCD-26MAR28-B55000",
    "side": "yes",
    "action": "buy",
    "quantity": "10",
    "price_dollars": "0.35"
  }
}`}
            response={`{
  "group_id": "grp_def456",
  "orders": [
    { "id": "ord_4", "client_order_id": "oco-leg1", "leg_role": "oco_leg", "status": "pending" },
    { "id": "ord_5", "client_order_id": "oco-leg2", "leg_role": "oco_leg", "status": "pending" }
  ]
}`}
            curl={`curl -X POST $HARMAN_URL/v1/groups/oco \\
  -H "Authorization: Bearer $HARMAN_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{"leg1":{"client_order_id":"oco-leg1","ticker":"KXBTCD-26MAR28-B50000","side":"yes","action":"buy","quantity":"10","price_dollars":"0.40"},"leg2":{"client_order_id":"oco-leg2","ticker":"KXBTCD-26MAR28-B55000","side":"yes","action":"buy","quantity":"10","price_dollars":"0.35"}}'`}
            notes="Both legs are placed immediately. When one fills, the other is cancelled."
          />
          <Endpoint
            method="GET"
            path="/v1/groups"
            scope="harman:read"
            description="List order groups for the current session."
            queryParams={[
              { name: "state", description: "Filter by group state: pending, active, completed, cancelled" },
            ]}
            response={`{
  "groups": [
    {
      "id": "grp_abc123",
      "type": "bracket",
      "state": "active",
      "created_at": "2026-03-01T12:00:00Z",
      "order_ids": ["ord_1", "ord_2", "ord_3"]
    }
  ]
}`}
            curl={`curl $HARMAN_URL/v1/groups?state=active \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/v1/groups/:id"
            scope="harman:read"
            description="Get a single order group with nested order details."
            response={`{
  "id": "grp_abc123",
  "type": "bracket",
  "state": "active",
  "created_at": "2026-03-01T12:00:00Z",
  "orders": [
    { "id": "ord_1", "leg_role": "entry", "state": "filled", "filled_quantity": "10" },
    { "id": "ord_2", "leg_role": "take_profit", "state": "resting" },
    { "id": "ord_3", "leg_role": "stop_loss", "state": "resting" }
  ]
}`}
            curl={`curl $HARMAN_URL/v1/groups/grp_abc123 \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="DELETE"
            path="/v1/groups/:id"
            scope="harman:write"
            description="Cancel all orders in a group."
            response={`{ "status": "cancelling" }`}
            curl={`curl -X DELETE $HARMAN_URL/v1/groups/grp_abc123 \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
        </Section>

        {/* ============================================================= */}
        {/* FILLS & AUDIT */}
        {/* ============================================================= */}
        <Section id="fills-audit" title="Fills & Audit">
          <Endpoint
            method="GET"
            path="/v1/fills"
            scope="harman:read"
            description="List fills for the current session."
            queryParams={[
              { name: "order_id", description: "Filter fills by order ID" },
            ]}
            response={`{
  "fills": [
    {
      "id": "fill_001",
      "order_id": "ord_abc123",
      "ticker": "KXBTCD-26MAR28-B50000",
      "side": "yes",
      "action": "buy",
      "quantity": "5",
      "price_dollars": "0.42",
      "filled_at": "2026-03-01T12:01:00Z"
    }
  ]
}`}
            curl={`curl "$HARMAN_URL/v1/fills?order_id=ord_abc123" \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/v1/audit"
            scope="harman:read"
            description="List audit log entries for order state transitions."
            queryParams={[
              { name: "order_id", description: "Filter audit entries by order ID" },
            ]}
            response={`{
  "entries": [
    {
      "id": "aud_001",
      "order_id": "ord_abc123",
      "from_state": "pending",
      "to_state": "resting",
      "reason": "exchange_ack",
      "timestamp": "2026-03-01T12:00:01Z"
    }
  ]
}`}
            curl={`curl "$HARMAN_URL/v1/audit?order_id=ord_abc123" \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
        </Section>

        {/* ============================================================= */}
        {/* OMS MARKET DATA */}
        {/* ============================================================= */}
        <Section id="oms-market-data" title="Market Data">
          <Endpoint
            method="GET"
            path="/v1/tickers"
            scope="harman:read"
            description="List all available tickers."
            response={`{
  "tickers": [
    "KXBTCD-26MAR28-B50000",
    "KXBTCD-26MAR28-B55000",
    "KXBTCD-26MAR28-B60000"
  ]
}`}
            curl={`curl $HARMAN_URL/v1/tickers \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/v1/snap"
            scope="harman:read"
            description="Get latest price snapshots for tickers."
            queryParams={[
              { name: "tickers", description: "Comma-separated ticker list (e.g. T1,T2). Omit for all." },
            ]}
            response={`{
  "feed": "kalshi",
  "snapshots": [
    {
      "ticker": "KXBTCD-26MAR28-B50000",
      "yes_bid": "0.42",
      "yes_ask": "0.44",
      "no_bid": "0.56",
      "no_ask": "0.58",
      "last_price": "0.43",
      "volume": 1234,
      "ts": "2026-03-01T12:00:00Z"
    }
  ],
  "count": 1
}`}
            curl={`curl "$HARMAN_URL/v1/snap?tickers=KXBTCD-26MAR28-B50000,KXBTCD-26MAR28-B55000" \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/v1/monitor/categories"
            scope="harman:read"
            description="Browse market categories (top level of the hierarchy)."
            response={`{
  "categories": [
    { "name": "Crypto", "event_count": 12, "series_count": 3 },
    { "name": "Economics", "event_count": 8, "series_count": 2 }
  ]
}`}
            curl={`curl $HARMAN_URL/v1/monitor/categories \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/v1/monitor/series"
            scope="harman:read"
            description="Browse series within a category."
            queryParams={[
              { name: "category", description: "Category name to filter by" },
            ]}
            response={`{
  "series": [
    { "ticker": "KXBTCD", "title": "Bitcoin Daily Close", "event_count": 30, "market_count": 150 }
  ]
}`}
            curl={`curl "$HARMAN_URL/v1/monitor/series?category=Crypto" \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/v1/monitor/events"
            scope="harman:read"
            description="Browse events within a series."
            queryParams={[
              { name: "series", description: "Series ticker to filter by" },
            ]}
            response={`{
  "events": [
    { "ticker": "KXBTCD-26MAR28", "title": "BTC Mar 28", "status": "active", "market_count": 5 }
  ]
}`}
            curl={`curl "$HARMAN_URL/v1/monitor/events?series=KXBTCD" \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/v1/monitor/markets"
            scope="harman:read"
            description="Browse markets within an event, with live prices."
            queryParams={[
              { name: "event", description: "Event ticker to filter by" },
            ]}
            response={`{
  "markets": [
    {
      "ticker": "KXBTCD-26MAR28-B50000",
      "title": "BTC above $50,000",
      "status": "active",
      "close_time": "2026-03-28T23:59:59Z",
      "yes_bid": "0.42",
      "yes_ask": "0.44",
      "volume": 1234
    }
  ]
}`}
            curl={`curl "$HARMAN_URL/v1/monitor/markets?event=KXBTCD-26MAR28" \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <div className="text-xs text-fg-subtle">
            Looking for 1-minute OHLCV bars or the parquet data archive? Those are served by the
            Market Data API — see{" "}
            <a href="#data-endpoints" className="text-accent hover:underline">Data</a>.
          </div>
        </Section>

        {/* ============================================================= */}
        {/* ACCOUNT */}
        {/* ============================================================= */}
        <Section id="account" title="Account">
          <Endpoint
            method="GET"
            path="/v1/info"
            scope="none (public)"
            description="Get instance information. No authentication required."
            response={`{
  "exchange": "kalshi",
  "environment": "prod",
  "version": "0.3.46"
}`}
            curl={`curl $HARMAN_URL/v1/info`}
          />
          <Endpoint
            method="GET"
            path="/v1/me"
            scope="harman:read"
            description="Get current user identity and session info."
            response={`{
  "key_prefix": "hk_abc",
  "scopes": ["harman:read", "harman:write"],
  "session_id": "sess_001",
  "exchange": "kalshi",
  "environment": "prod",
  "email": "trader@example.com"
}`}
            curl={`curl $HARMAN_URL/v1/me \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/health"
            scope="none (public)"
            description="Health check endpoint. No authentication required."
            response={`{ "status": "ok", "suspended": false }`}
            curl={`curl $HARMAN_URL/health`}
          />
        </Section>

        {/* ============================================================= */}
        {/* ADMIN */}
        {/* ============================================================= */}
        <Section id="admin" title="Admin">
          <Endpoint
            method="GET"
            path="/v1/admin/positions"
            scope="harman:admin"
            description="Get positions for the current session, or all sessions."
            queryParams={[
              { name: "all", description: "Set to true to return positions across all sessions" },
            ]}
            response={`{
  "exchange": [
    { "ticker": "KXBTCD-26MAR28-B50000", "side": "yes", "quantity": "10", "avg_price": "0.42" }
  ],
  "local": [
    { "ticker": "KXBTCD-26MAR28-B50000", "side": "yes", "net_quantity": "10" }
  ]
}`}
            curl={`curl "$HARMAN_URL/v1/admin/positions?all=true" \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/v1/admin/risk"
            scope="harman:admin"
            description="Get current risk limits and usage."
            response={`{
  "max_notional": "5000.00",
  "global_max_notional": "10000.00",
  "open_notional": "420.00",
  "available_notional": "4580.00",
  "session_id": "sess_001"
}`}
            curl={`curl $HARMAN_URL/v1/admin/risk \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/v1/admin/sessions"
            scope="harman:admin"
            description="List all active sessions."
            response={`{
  "sessions": [
    { "id": "sess_001", "email": "trader@example.com", "exchange": "kalshi", "environment": "prod" }
  ]
}`}
            curl={`curl $HARMAN_URL/v1/admin/sessions \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/v1/admin/users"
            scope="harman:admin"
            description="List all API keys and sessions."
            response={`{
  "keys": [
    { "prefix": "hk_abc", "email": "trader@example.com", "scopes": ["harman:read", "harman:write"] }
  ],
  "sessions": [
    { "id": "sess_001", "key_prefix": "hk_abc", "exchange": "kalshi" }
  ]
}`}
            curl={`curl $HARMAN_URL/v1/admin/users \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="PUT"
            path="/v1/admin/sessions/:id/risk"
            scope="harman:admin"
            description="Update risk limits for a session."
            body={`{ "max_notional": "10000.00" }`}
            response={`{ "session_id": "sess_001", "max_notional": "10000.00" }`}
            curl={`curl -X PUT $HARMAN_URL/v1/admin/sessions/sess_001/risk \\
  -H "Authorization: Bearer $HARMAN_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{"max_notional":"10000.00"}'`}
          />
          <Endpoint
            method="PUT"
            path="/v1/admin/sessions/:id/resume"
            scope="harman:admin"
            description="Resume a suspended session."
            response={`{ "session_id": "sess_001", "resumed": true }`}
            curl={`curl -X PUT $HARMAN_URL/v1/admin/sessions/sess_001/resume \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="POST"
            path="/v1/admin/mass-cancel"
            scope="harman:admin"
            description="Cancel all open orders across all sessions. Requires confirmation."
            body={`{ "confirm": true }`}
            response={`{ "cancelled": 42 }`}
            curl={`curl -X POST $HARMAN_URL/v1/admin/mass-cancel \\
  -H "Authorization: Bearer $HARMAN_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{"confirm":true}'`}
          />
          <Endpoint
            method="POST"
            path="/v1/admin/pump"
            scope="harman:admin"
            description="Trigger a pump cycle (re-sync pending orders with the exchange)."
            response={`{ "pumped": 3 }`}
            curl={`curl -X POST $HARMAN_URL/v1/admin/pump \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="POST"
            path="/v1/admin/reconcile"
            scope="harman:admin"
            description="Trigger position reconciliation with the exchange."
            response={`{ "reconciled": true, "mismatches": 0 }`}
            curl={`curl -X POST $HARMAN_URL/v1/admin/reconcile \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="POST"
            path="/v1/admin/resume"
            scope="harman:admin"
            description="Resume the global OMS (un-suspend)."
            response={`{ "resumed": true }`}
            curl={`curl -X POST $HARMAN_URL/v1/admin/resume \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="POST"
            path="/v1/admin/cache/invalidate"
            scope="harman:admin"
            description="Invalidate the server-side cache (secmaster, snap, etc.)."
            response={`{ "cleared": true }`}
            curl={`curl -X POST $HARMAN_URL/v1/admin/cache/invalidate \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="GET"
            path="/v1/admin/settlements"
            scope="harman:admin"
            description="List all settlement records for the current session. Shows market settlement outcomes, revenue, and fees for settled contracts."
            response={`{
  "settlements": [
    {
      "id": 1,
      "session_id": 26,
      "ticker": "KXBTCD-26MAR0200-B96749.99",
      "event_ticker": "KXBTCD-26MAR0200",
      "market_result": "no",
      "yes_count": "0",
      "no_count": "0",
      "revenue_dollars": "0.00",
      "settled_time": "2026-03-02T02:00:00Z",
      "fee_cost_dollars": "0.00",
      "value_dollars": "0"
    }
  ]
}`}
            curl={`curl $HARMAN_URL/v1/admin/settlements \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
        </Section>

        {/* ============================================================= */}
        {/* TYPES */}
        {/* ============================================================= */}
        <Section id="types" title="Types">
          <div className="space-y-4">
            <TypeTable
              name="Side"
              values={[
                { value: '"yes"', description: "Yes side of the contract" },
                { value: '"no"', description: "No side of the contract" },
              ]}
            />
            <TypeTable
              name="Action"
              values={[
                { value: '"buy"', description: "Buy (open or increase position)" },
                { value: '"sell"', description: "Sell (close or decrease position)" },
              ]}
            />
            <TypeTable
              name="TimeInForce"
              values={[
                { value: '"gtc"', description: "Good til cancelled (default)" },
                { value: '"ioc"', description: "Immediate or cancel" },
              ]}
            />
            <TypeTable
              name="OrderState"
              values={[
                { value: '"pending"', description: "Submitted, awaiting exchange acknowledgment" },
                { value: '"resting"', description: "Acknowledged and resting on the book" },
                { value: '"partially_filled"', description: "Some quantity filled, remainder resting" },
                { value: '"filled"', description: "Fully filled" },
                { value: '"pending_cancel"', description: "Cancel request sent to exchange" },
                { value: '"cancelled"', description: "Successfully cancelled" },
                { value: '"pending_amend"', description: "Amend request sent to exchange" },
                { value: '"pending_decrease"', description: "Decrease request sent to exchange" },
                { value: '"rejected"', description: "Rejected by exchange" },
                { value: '"expired"', description: "Expired (e.g. IOC not filled)" },
                { value: '"suspended"', description: "Held due to session suspension" },
                { value: '"error"', description: "Internal error" },
              ]}
            />
            <div className="border border-border rounded-lg p-4 bg-bg-raised">
              <p className="text-sm font-semibold text-fg mb-2">State Groups (for ?state= filter)</p>
              <div className="space-y-1 text-xs text-fg-muted font-mono">
                <p><span className="text-accent">open</span> = pending, resting, partially_filled, pending_cancel, pending_amend, pending_decrease</p>
                <p><span className="text-accent">terminal</span> = filled, cancelled, rejected, expired, error</p>
                <p><span className="text-accent">resting</span> = resting, partially_filled</p>
                <p><span className="text-accent">today</span> = all orders created today (UTC)</p>
              </div>
            </div>
            <TypeTable
              name="GroupType"
              values={[
                { value: '"bracket"', description: "Entry + take-profit + stop-loss" },
                { value: '"oco"', description: "One-cancels-other (two legs)" },
              ]}
            />
            <TypeTable
              name="GroupState"
              values={[
                { value: '"pending"', description: "Group created, entry not yet filled (bracket)" },
                { value: '"active"', description: "Exit legs are live (bracket) or both legs live (OCO)" },
                { value: '"completed"', description: "Group fully resolved" },
                { value: '"cancelled"', description: "Group cancelled" },
              ]}
            />
            <TypeTable
              name="LegRole"
              values={[
                { value: '"entry"', description: "Bracket entry leg" },
                { value: '"take_profit"', description: "Bracket take-profit exit" },
                { value: '"stop_loss"', description: "Bracket stop-loss exit" },
                { value: '"oco_leg"', description: "OCO leg" },
                { value: "null", description: "Standalone order (not part of a group)" },
              ]}
            />
          </div>
        </Section>

        {/* ============================================================= */}
        {/* MARKET DATA API (api.varshtat.com) */}
        {/* ============================================================= */}
        <DataApiSections />

        {/* ============================================================= */}
        {/* FEEDS & PROTOCOLS */}
        {/* ============================================================= */}
        <FeedsProtocolsSections />
      </main>
    </div>
  );
}
