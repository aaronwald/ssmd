"use client";

import { useState } from "react";

// ---------------------------------------------------------------------------
// Copy button
// ---------------------------------------------------------------------------

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      onClick={() => {
        navigator.clipboard.writeText(text);
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      }}
      className="absolute top-2 right-2 text-xs px-2 py-1 rounded bg-bg-surface text-fg-muted hover:text-fg transition-colors"
      title="Copy to clipboard"
    >
      {copied ? "Copied" : "Copy"}
    </button>
  );
}

// ---------------------------------------------------------------------------
// Method badge
// ---------------------------------------------------------------------------

const methodColors: Record<string, string> = {
  GET: "bg-green-900/50 text-green-400",
  POST: "bg-blue-900/50 text-blue-400",
  PUT: "bg-amber-900/50 text-amber-400",
  DELETE: "bg-red-900/50 text-red-400",
};

function MethodBadge({ method }: { method: string }) {
  return (
    <span
      className={`inline-block text-xs font-mono font-bold px-2 py-0.5 rounded ${methodColors[method] ?? "bg-bg-surface text-fg-muted"}`}
    >
      {method}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Curl block
// ---------------------------------------------------------------------------

function CurlBlock({ curl }: { curl: string }) {
  return (
    <div className="relative mt-2">
      <CopyButton text={curl} />
      <pre className="bg-bg font-mono text-xs text-fg-muted p-3 rounded border border-border overflow-x-auto whitespace-pre-wrap">
        {curl}
      </pre>
    </div>
  );
}

// ---------------------------------------------------------------------------
// JSON block
// ---------------------------------------------------------------------------

function JsonBlock({ label, json }: { label: string; json: string }) {
  return (
    <div className="mt-2">
      <p className="text-xs text-fg-muted mb-1">{label}</p>
      <pre className="bg-bg font-mono text-xs text-fg-muted p-3 rounded border border-border overflow-x-auto whitespace-pre-wrap">
        {json}
      </pre>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Endpoint
// ---------------------------------------------------------------------------

interface EndpointProps {
  method: string;
  path: string;
  scope?: string;
  description: string;
  queryParams?: { name: string; description: string }[];
  body?: string;
  response: string;
  curl: string;
  notes?: string;
}

function Endpoint({
  method,
  path,
  scope,
  description,
  queryParams,
  body,
  response,
  curl,
  notes,
}: EndpointProps) {
  const id = `${method.toLowerCase()}-${path.replace(/[/:]/g, "-").replace(/^-+|-+$/g, "")}`;
  return (
    <div id={id} className="border border-border rounded-lg p-4 bg-bg-raised scroll-mt-20">
      <div className="flex items-center gap-3 flex-wrap">
        <MethodBadge method={method} />
        <code className="font-mono text-sm text-fg">{path}</code>
        {scope && (
          <span className="text-xs text-fg-subtle font-mono">({scope})</span>
        )}
      </div>
      <p className="text-sm text-fg-muted mt-2">{description}</p>
      {notes && <p className="text-xs text-fg-subtle mt-1">{notes}</p>}
      {queryParams && queryParams.length > 0 && (
        <div className="mt-3">
          <p className="text-xs font-semibold text-fg-muted mb-1">Query Parameters</p>
          <div className="space-y-1">
            {queryParams.map((qp) => (
              <div key={qp.name} className="text-xs text-fg-muted">
                <code className="font-mono text-accent">{qp.name}</code> &mdash; {qp.description}
              </div>
            ))}
          </div>
        </div>
      )}
      {body && <JsonBlock label="Request Body" json={body} />}
      <JsonBlock label="Response" json={response} />
      <CurlBlock curl={curl} />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Section
// ---------------------------------------------------------------------------

function Section({
  id,
  title,
  children,
}: {
  id: string;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <details id={id} open className="scroll-mt-20">
      <summary className="text-lg font-bold text-fg cursor-pointer select-none py-2 border-b border-border mb-4">
        {title}
      </summary>
      <div className="space-y-4 pb-8">{children}</div>
    </details>
  );
}

// ---------------------------------------------------------------------------
// TOC
// ---------------------------------------------------------------------------

const tocSections = [
  { id: "authentication", label: "Authentication" },
  { id: "orders", label: "Orders" },
  { id: "order-groups", label: "Order Groups" },
  { id: "fills-audit", label: "Fills & Audit" },
  { id: "market-data", label: "Market Data" },
  { id: "account", label: "Account" },
  { id: "admin", label: "Admin" },
  { id: "types", label: "Types" },
];

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
        <ul className="space-y-1.5">
          {tocSections.map((s) => (
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
      </nav>

      <main className="min-w-0 flex-1">
        <h1 className="text-2xl font-bold text-fg mb-1">API Reference</h1>
        <p className="text-sm text-fg-muted mb-6">
          Harman OMS REST API. All prices are in dollars as decimal strings (e.g.{" "}
          <code className="font-mono text-accent">&quot;0.42&quot;</code> = 42 cents). Quantities
          are decimal strings. Base URL depends on deployment.
        </p>

        {/* Mobile TOC */}
        <div className="lg:hidden mb-6 flex flex-wrap gap-2">
          {tocSections.map((s) => (
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
            <p>The API supports four authentication methods. Include credentials on every request.</p>
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
        {/* MARKET DATA */}
        {/* ============================================================= */}
        <Section id="market-data" title="Market Data">
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
          <Endpoint
            method="GET"
            path="/v1/search"
            scope="harman:read"
            description="Search markets by text query."
            queryParams={[
              { name: "q", description: "Search query string" },
              { name: "limit", description: "Max results to return (default: 20)" },
            ]}
            response={`{
  "results": [
    { "ticker": "KXBTCD-26MAR28-B50000", "title": "BTC above $50,000", "category": "Crypto" }
  ]
}`}
            curl={`curl "$HARMAN_URL/v1/search?q=bitcoin&limit=10" \\
  -H "Authorization: Bearer $HARMAN_TOKEN"`}
          />
          <Endpoint
            method="POST"
            path="/v1/watchlist"
            scope="harman:write"
            description="Update the watchlist for the current session."
            body={`{
  "tickers": ["KXBTCD-26MAR28-B50000", "KXBTCD-26MAR28-B55000"]
}`}
            response={`{ "tickers": ["KXBTCD-26MAR28-B50000", "KXBTCD-26MAR28-B55000"] }`}
            curl={`curl -X POST $HARMAN_URL/v1/watchlist \\
  -H "Authorization: Bearer $HARMAN_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{"tickers":["KXBTCD-26MAR28-B50000","KXBTCD-26MAR28-B55000"]}'`}
          />
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
      </main>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Type table helper
// ---------------------------------------------------------------------------

function TypeTable({
  name,
  values,
}: {
  name: string;
  values: { value: string; description: string }[];
}) {
  return (
    <div className="border border-border rounded-lg p-4 bg-bg-raised">
      <p className="text-sm font-semibold text-fg mb-2">{name}</p>
      <table className="w-full text-xs">
        <thead>
          <tr className="text-left text-fg-muted border-b border-border">
            <th className="pb-1 pr-4 font-medium">Value</th>
            <th className="pb-1 font-medium">Description</th>
          </tr>
        </thead>
        <tbody>
          {values.map((v) => (
            <tr key={v.value} className="border-b border-border last:border-0">
              <td className="py-1.5 pr-4 font-mono text-accent">{v.value}</td>
              <td className="py-1.5 text-fg-muted">{v.description}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
