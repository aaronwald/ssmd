"use client";

import { useState, useMemo, useCallback, useEffect } from "react";
import {
  useSecmasterStats,
  useSecmasterMarkets,
  useSecmasterPairs,
  useSecmasterConditions,
  useMe,
} from "@/lib/hooks";
import type {
  SecmasterStats,
  SecmasterMarket,
  SecmasterPair,
  SecmasterCondition,
} from "@/lib/types";

type Exchange = "kalshi" | "kraken" | "polymarket";

/** Read URL search params directly (avoids useSearchParams Suspense) */
function getUrlParam(key: string): string | null {
  if (typeof window === "undefined") return null;
  return new URLSearchParams(window.location.search).get(key);
}

function setUrlParams(updates: Record<string, string | null>) {
  if (typeof window === "undefined") return;
  const params = new URLSearchParams(window.location.search);
  for (const [k, v] of Object.entries(updates)) {
    if (v) params.set(k, v);
    else params.delete(k);
  }
  const qs = params.toString();
  const url = qs ? `${window.location.pathname}?${qs}` : window.location.pathname;
  window.history.replaceState(null, "", url);
}

function fmtNum(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toLocaleString();
}

function fmtCloseTime(dateStr: string | null): string {
  if (!dateStr) return "-";
  const d = new Date(dateStr);
  return d.toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    timeZone: "America/New_York",
  }) + " EST";
}

function StatusBadge({ status }: { status: string }) {
  let cls = "bg-fg-subtle/15 text-fg-subtle";
  if (status === "active") cls = "bg-green/15 text-green";
  else if (status === "closed" || status === "halted") cls = "bg-fg-subtle/15 text-fg-subtle";
  else if (status === "settled" || status === "resolved") cls = "bg-blue/15 text-blue";
  else if (status === "delisted") cls = "bg-red/15 text-red";
  return (
    <span className={`text-xs px-1.5 py-0.5 rounded font-medium ${cls}`}>
      {status}
    </span>
  );
}

export default function SecmasterPage() {
  const { data: me } = useMe();
  const hasAdmin = me?.scopes.includes("harman:admin") || me?.scopes.includes("*") || me?.scopes.includes("secmaster:read");

  if (!me) return <div className="py-10 text-center text-fg-muted">Loading...</div>;
  if (!hasAdmin) return <div className="py-10 text-center text-fg-muted">Requires <code className="font-mono text-accent">secmaster:read</code> scope.</div>;

  return <SecmasterContent />;
}

function SecmasterContent() {
  const { data: stats } = useSecmasterStats();
  const [exchange, setExchange] = useState<Exchange>(() => (getUrlParam("exchange") as Exchange) || "kalshi");

  const selectExchange = useCallback((ex: Exchange) => {
    setExchange(ex);
    setUrlParams({ exchange: ex, status: null, series: null, category: null, base: null, market_type: null });
  }, []);

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-bold">Secmaster</h1>

      {stats && <StatsBar stats={stats} />}

      <ExchangeTabs selected={exchange} onSelect={selectExchange} />

      {exchange === "kalshi" && <KalshiTable stats={stats} />}
      {exchange === "kraken" && <KrakenTable stats={stats} />}
      {exchange === "polymarket" && <PolymarketTable stats={stats} />}
    </div>
  );
}

// --- Stats bar ---

function StatsBar({ stats }: { stats: SecmasterStats }) {
  const kalshiActive = stats.markets.by_status?.active ?? 0;
  const kalshiTotal = stats.markets.total;
  const krakenTotal = stats.pairs.total;
  const krakenActive = stats.pairs.by_market_type?.perpetual ?? 0;
  const pmActive = stats.conditions.by_status?.active ?? 0;
  const pmTotal = stats.conditions.total;

  return (
    <div className="flex flex-wrap gap-4">
      <StatCard label="Kalshi Markets" value={`${fmtNum(kalshiActive)} active`} sub={`${fmtNum(kalshiTotal)} total`} />
      <StatCard label="Kraken Pairs" value={`${fmtNum(krakenActive)} perp`} sub={`${fmtNum(krakenTotal)} total`} />
      <StatCard label="Polymarket" value={`${fmtNum(pmActive)} active`} sub={`${fmtNum(pmTotal)} total`} />
      <StatCard label="Events" value={fmtNum(stats.events.total)} sub={`${stats.events.by_status?.active ?? 0} active`} />
    </div>
  );
}

function StatCard({ label, value, sub }: { label: string; value: string; sub: string }) {
  return (
    <div className="bg-bg-raised border border-border rounded-lg px-4 py-3 min-w-[160px]">
      <div className="text-xs text-fg-muted">{label}</div>
      <div className="text-lg font-bold text-fg mt-0.5">{value}</div>
      <div className="text-xs text-fg-subtle">{sub}</div>
    </div>
  );
}

// --- Exchange tabs ---

function ExchangeTabs({ selected, onSelect }: { selected: Exchange; onSelect: (ex: Exchange) => void }) {
  const tabs: { id: Exchange; label: string }[] = [
    { id: "kalshi", label: "Kalshi" },
    { id: "kraken", label: "Kraken" },
    { id: "polymarket", label: "Polymarket" },
  ];

  return (
    <div className="flex gap-2">
      {tabs.map((t) => (
        <button
          key={t.id}
          onClick={() => onSelect(t.id)}
          className={`px-4 py-1.5 rounded-full text-sm font-medium transition-colors ${
            selected === t.id
              ? "bg-accent text-bg"
              : "bg-bg-raised border border-border text-fg-muted hover:text-fg hover:border-fg-subtle"
          }`}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}

// --- Filter bar helper ---

function FilterSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: { value: string; label: string }[];
  onChange: (v: string) => void;
}) {
  return (
    <label className="flex items-center gap-1.5 text-xs text-fg-muted">
      {label}
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="bg-bg-raised border border-border rounded px-2 py-1 text-xs text-fg font-mono focus:outline-none focus:border-accent"
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>{o.label}</option>
        ))}
      </select>
    </label>
  );
}

// --- Kalshi table ---

function KalshiTable({ stats }: { stats: SecmasterStats | undefined }) {
  const [status, setStatus] = useState(() => getUrlParam("status") || "active");
  const [series, setSeries] = useState(() => getUrlParam("series") || "");
  const [category, setCategory] = useState(() => getUrlParam("category") || "");
  const [closeHours, setCloseHours] = useState(() => getUrlParam("close_within_hours") || "");
  const [limit, setLimit] = useState(100);

  useEffect(() => {
    setUrlParams({ status: status || null, series: series || null, category: category || null, close_within_hours: closeHours || null });
  }, [status, series, category, closeHours]);

  const filters = useMemo(() => ({
    status: status || undefined,
    series: series || undefined,
    category: category || undefined,
    close_within_hours: closeHours ? parseInt(closeHours) : undefined,
    limit,
  }), [status, series, category, closeHours, limit]);

  const { data: markets, error, isLoading } = useSecmasterMarkets(filters);

  const categories = useMemo(() => {
    if (!stats) return [];
    return Object.keys(stats.events.by_category).sort();
  }, [stats]);

  const statusOptions = [
    { value: "", label: "All" },
    { value: "active", label: "Active" },
    { value: "closed", label: "Closed" },
    { value: "settled", label: "Settled" },
  ];

  const categoryOptions = useMemo(() => [
    { value: "", label: "All Categories" },
    ...categories.map((c) => ({ value: c, label: c })),
  ], [categories]);

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-3">
        <FilterSelect label="Status" value={status} options={statusOptions} onChange={setStatus} />
        <FilterSelect label="Category" value={category} options={categoryOptions} onChange={setCategory} />
        <label className="flex items-center gap-1.5 text-xs text-fg-muted">
          Series
          <input
            type="text"
            value={series}
            onChange={(e) => setSeries(e.target.value.toUpperCase())}
            placeholder="e.g. KXBTCD"
            className="bg-bg-raised border border-border rounded px-2 py-1 text-xs text-fg font-mono w-24 focus:outline-none focus:border-accent"
          />
        </label>
        <label className="flex items-center gap-1.5 text-xs text-fg-muted">
          Close within
          <input
            type="number"
            value={closeHours}
            onChange={(e) => setCloseHours(e.target.value)}
            placeholder="hours"
            className="bg-bg-raised border border-border rounded px-2 py-1 text-xs text-fg font-mono w-16 focus:outline-none focus:border-accent"
          />
          <span>h</span>
        </label>
      </div>

      {error && <p className="text-sm text-red">Error: {error.message}</p>}
      {isLoading && !markets && <p className="text-sm text-fg-muted">Loading...</p>}

      {markets && (
        <>
          <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-xs text-fg-muted border-b border-border">
                    <th className="px-4 py-2">Ticker</th>
                    <th className="px-4 py-2">Title</th>
                    <th className="px-4 py-2">Status</th>
                    <th className="px-4 py-2">Series</th>
                    <th className="px-4 py-2">Category</th>
                    <th className="px-4 py-2">Close</th>
                    <th className="px-4 py-2 text-right">Volume</th>
                  </tr>
                </thead>
                <tbody>
                  {markets.map((m) => (
                    <tr key={m.ticker} className="border-b border-border-subtle hover:bg-bg-surface-hover">
                      <td className="px-4 py-1.5 font-mono text-xs text-fg">{m.ticker}</td>
                      <td className="px-4 py-1.5 text-xs text-fg-muted max-w-[300px] truncate">{m.title}</td>
                      <td className="px-4 py-1.5"><StatusBadge status={m.status} /></td>
                      <td className="px-4 py-1.5 font-mono text-xs text-fg-subtle">{m.series_ticker}</td>
                      <td className="px-4 py-1.5 text-xs text-fg-subtle">{m.category}</td>
                      <td className="px-4 py-1.5 text-xs text-fg-muted whitespace-nowrap">{fmtCloseTime(m.close_time)}</td>
                      <td className="px-4 py-1.5 font-mono text-xs text-fg-subtle text-right">{m.volume != null ? fmtNum(m.volume) : "-"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
          <div className="flex items-center justify-between text-xs text-fg-muted">
            <span>Showing {markets.length}{markets.length >= limit ? ` of ${limit}+` : ""}</span>
            {markets.length >= limit && (
              <button
                onClick={() => setLimit((l) => l + 100)}
                className="px-3 py-1 rounded border border-border text-fg-muted hover:text-fg hover:border-fg-subtle transition-colors"
              >
                Load More
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
}

// --- Kraken table ---

function KrakenTable({ stats }: { stats: SecmasterStats | undefined }) {
  const [status, setStatus] = useState(() => getUrlParam("status") || "active");
  const [base, setBase] = useState(() => getUrlParam("base") || "");
  const [marketType, setMarketType] = useState(() => getUrlParam("market_type") || "");
  const [limit, setLimit] = useState(100);

  useEffect(() => {
    setUrlParams({ status: status || null, base: base || null, market_type: marketType || null });
  }, [status, base, marketType]);

  const filters = useMemo(() => ({
    status: status || undefined,
    base: base || undefined,
    market_type: marketType || undefined,
    limit,
  }), [status, base, marketType, limit]);

  const { data: pairs, error, isLoading } = useSecmasterPairs(filters);

  const statusOptions = [
    { value: "", label: "All" },
    { value: "active", label: "Active" },
    { value: "halted", label: "Halted" },
    { value: "delisted", label: "Delisted" },
  ];

  const typeOptions = [
    { value: "", label: "All Types" },
    { value: "perpetual", label: "Perpetual" },
    { value: "fixed_maturity", label: "Fixed Maturity" },
  ];

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-3">
        <FilterSelect label="Status" value={status} options={statusOptions} onChange={setStatus} />
        <FilterSelect label="Type" value={marketType} options={typeOptions} onChange={setMarketType} />
        <label className="flex items-center gap-1.5 text-xs text-fg-muted">
          Base
          <input
            type="text"
            value={base}
            onChange={(e) => setBase(e.target.value.toUpperCase())}
            placeholder="e.g. BTC"
            className="bg-bg-raised border border-border rounded px-2 py-1 text-xs text-fg font-mono w-20 focus:outline-none focus:border-accent"
          />
        </label>
      </div>

      {error && <p className="text-sm text-red">Error: {error.message}</p>}
      {isLoading && !pairs && <p className="text-sm text-fg-muted">Loading...</p>}

      {pairs && (
        <>
          <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-xs text-fg-muted border-b border-border">
                    <th className="px-4 py-2">Pair ID</th>
                    <th className="px-4 py-2">Symbol</th>
                    <th className="px-4 py-2">Base</th>
                    <th className="px-4 py-2">Quote</th>
                    <th className="px-4 py-2">Type</th>
                    <th className="px-4 py-2">Status</th>
                  </tr>
                </thead>
                <tbody>
                  {pairs.map((p) => (
                    <tr key={p.pair_id} className="border-b border-border-subtle hover:bg-bg-surface-hover">
                      <td className="px-4 py-1.5 font-mono text-xs text-fg">{p.pair_id}</td>
                      <td className="px-4 py-1.5 font-mono text-xs text-fg-muted">{p.symbol}</td>
                      <td className="px-4 py-1.5 font-mono text-xs text-fg">{p.base}</td>
                      <td className="px-4 py-1.5 font-mono text-xs text-fg-subtle">{p.quote}</td>
                      <td className="px-4 py-1.5 text-xs text-fg-subtle">{p.market_type}</td>
                      <td className="px-4 py-1.5"><StatusBadge status={p.status} /></td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
          <div className="flex items-center justify-between text-xs text-fg-muted">
            <span>Showing {pairs.length}{pairs.length >= limit ? ` of ${limit}+` : ""}</span>
            {pairs.length >= limit && (
              <button
                onClick={() => setLimit((l) => l + 100)}
                className="px-3 py-1 rounded border border-border text-fg-muted hover:text-fg hover:border-fg-subtle transition-colors"
              >
                Load More
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
}

// --- Polymarket table ---

function PolymarketTable({ stats }: { stats: SecmasterStats | undefined }) {
  const [status, setStatus] = useState(() => getUrlParam("status") || "active");
  const [category, setCategory] = useState(() => getUrlParam("category") || "");
  const [limit, setLimit] = useState(100);

  useEffect(() => {
    setUrlParams({ status: status || null, category: category || null });
  }, [status, category]);

  const filters = useMemo(() => ({
    status: status || undefined,
    category: category || undefined,
    limit,
  }), [status, category, limit]);

  const { data: conditions, error, isLoading } = useSecmasterConditions(filters);

  const categories = useMemo(() => {
    if (!stats) return [];
    return Object.keys(stats.conditions.by_category).sort();
  }, [stats]);

  const statusOptions = [
    { value: "", label: "All" },
    { value: "active", label: "Active" },
    { value: "resolved", label: "Resolved" },
  ];

  const categoryOptions = useMemo(() => [
    { value: "", label: "All Categories" },
    ...categories.map((c) => ({ value: c, label: c })),
  ], [categories]);

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-3">
        <FilterSelect label="Status" value={status} options={statusOptions} onChange={setStatus} />
        <FilterSelect label="Category" value={category} options={categoryOptions} onChange={setCategory} />
      </div>

      {error && <p className="text-sm text-red">Error: {error.message}</p>}
      {isLoading && !conditions && <p className="text-sm text-fg-muted">Loading...</p>}

      {conditions && (
        <>
          <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-xs text-fg-muted border-b border-border">
                    <th className="px-4 py-2">Condition ID</th>
                    <th className="px-4 py-2">Question</th>
                    <th className="px-4 py-2">Status</th>
                    <th className="px-4 py-2">Category</th>
                    <th className="px-4 py-2 text-right">Tokens</th>
                    <th className="px-4 py-2">End Date</th>
                  </tr>
                </thead>
                <tbody>
                  {conditions.map((c) => (
                    <tr key={c.condition_id} className="border-b border-border-subtle hover:bg-bg-surface-hover">
                      <td className="px-4 py-1.5 font-mono text-xs text-fg max-w-[200px] truncate" title={c.condition_id}>{c.condition_id.slice(0, 10)}...</td>
                      <td className="px-4 py-1.5 text-xs text-fg-muted max-w-[400px] truncate" title={c.question}>{c.question}</td>
                      <td className="px-4 py-1.5"><StatusBadge status={c.status} /></td>
                      <td className="px-4 py-1.5 text-xs text-fg-subtle">{c.category || "-"}</td>
                      <td className="px-4 py-1.5 font-mono text-xs text-fg-subtle text-right">{c.token_count}</td>
                      <td className="px-4 py-1.5 text-xs text-fg-muted whitespace-nowrap">{fmtCloseTime(c.end_date)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
          <div className="flex items-center justify-between text-xs text-fg-muted">
            <span>Showing {conditions.length}{conditions.length >= limit ? ` of ${limit}+` : ""}</span>
            {conditions.length >= limit && (
              <button
                onClick={() => setLimit((l) => l + 100)}
                className="px-3 py-1 rounded border border-border text-fg-muted hover:text-fg hover:border-fg-subtle transition-colors"
              >
                Load More
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
}
