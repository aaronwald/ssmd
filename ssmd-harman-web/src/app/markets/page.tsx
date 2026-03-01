"use client";

import { Suspense, useState, useMemo, useCallback } from "react";
import { useSearchParams, useRouter } from "next/navigation";
import { useCategories, useSeries, useEvents, useMarkets } from "@/lib/hooks";
import type { MonitorCategory, MonitorSeries, MonitorEvent } from "@/lib/types";

type SortKey = "ticker" | "title" | "yes_bid" | "yes_ask" | "last" | "volume" | "close_time";
type SortDir = "asc" | "desc";
type Exchange = "" | "kalshi" | "kraken" | "polymarket";

/** Detect exchange from field names present on monitor objects. */
function categoryExchange(cat: MonitorCategory): Exchange | null {
  if (cat.base_count != null || cat.instrument_count != null) return "kraken";
  if (cat.pm_condition_count != null) return "polymarket";
  if (cat.event_count != null || cat.series_count != null) return "kalshi";
  return null;
}

function seriesExchange(s: MonitorSeries): Exchange {
  if (s.active_pairs != null) return "kraken";
  if (s.active_conditions != null) return "polymarket";
  return "kalshi";
}

function categoryCount(c: MonitorCategory): string {
  if (c.event_count != null) return `${c.event_count} events`;
  if (c.instrument_count != null) return `${c.instrument_count} instruments`;
  if (c.pm_condition_count != null) return `${c.pm_condition_count} conditions`;
  return "";
}

function seriesCount(s: MonitorSeries): string {
  if (s.active_events != null) return `${s.active_events} events`;
  if (s.active_pairs != null) return `${s.active_pairs} pairs`;
  if (s.active_conditions != null) return `${s.active_conditions} conditions`;
  return "0";
}

function eventCount(ev: MonitorEvent): string {
  if (ev.market_count != null) return `${ev.market_count} markets`;
  if (ev.pair_count != null) return `${ev.pair_count} pairs`;
  if (ev.token_count != null) return `${ev.token_count} tokens`;
  return "0";
}

export default function MarketsPage() {
  return (
    <Suspense fallback={<div className="p-8 text-center text-fg-subtle">Loading...</div>}>
      <MarketsContent />
    </Suspense>
  );
}

function MarketsContent() {
  const searchParams = useSearchParams();
  const router = useRouter();

  // Read filter state from URL params
  const exchange = (searchParams.get("exchange") ?? "") as Exchange;
  const category = searchParams.get("category");
  const series = searchParams.get("series");
  const event = searchParams.get("event");
  const search = searchParams.get("q") ?? "";
  const [sortKey, setSortKey] = useState<SortKey>("ticker");
  const [sortDir, setSortDir] = useState<SortDir>("asc");

  const { data: categories } = useCategories();
  const { data: seriesList } = useSeries(category);
  const { data: events } = useEvents(series);
  const { data: markets, error } = useMarkets(event);

  // Filter categories by exchange.
  // Categories can be exclusive (Kraken Futures) or shared (Crypto has both Kalshi + PM series).
  // For shared categories we keep them and filter at the series level.
  const filteredCategories = useMemo(() => {
    if (!categories) return undefined;
    if (!exchange) return categories;
    return categories.filter((c) => {
      const ex = categoryExchange(c);
      if (ex === null) return true; // unknown shape, keep
      if (ex === exchange) return true; // exact match
      // For kalshi: Crypto category has pm_condition_count but also Kalshi series inside.
      // Show it — series filtering will handle the rest.
      if (exchange === "kalshi" && ex === "polymarket") return true;
      if (exchange === "polymarket" && ex === "kalshi") return true;
      return false;
    });
  }, [categories, exchange]);

  // Filter series by exchange (categories are shared, e.g. Crypto has both Kalshi and PM series)
  const filteredSeries = useMemo(() => {
    if (!seriesList) return undefined;
    if (!exchange) return seriesList;
    return seriesList.filter((s) => seriesExchange(s) === exchange);
  }, [seriesList, exchange]);

  // Update URL params helper
  const setParams = useCallback((updates: Record<string, string | null>) => {
    const params = new URLSearchParams(searchParams.toString());
    for (const [k, v] of Object.entries(updates)) {
      if (v) params.set(k, v);
      else params.delete(k);
    }
    router.replace(`/markets?${params.toString()}`);
  }, [searchParams, router]);

  const handleExchangeChange = (val: string) => {
    setParams({ exchange: val || null, category: null, series: null, event: null, q: null });
  };

  const handleCategoryChange = (val: string) => {
    setParams({ category: val || null, series: null, event: null, q: null });
  };

  const handleSeriesChange = (val: string) => {
    setParams({ series: val || null, event: null, q: null });
  };

  const handleEventChange = (val: string) => {
    setParams({ event: val || null, q: null });
  };

  const handleSearchChange = (val: string) => {
    setParams({ q: val || null });
  };

  // Filter + sort markets
  const filtered = useMemo(() => {
    if (!markets) return undefined;
    let result = markets;
    if (search) {
      const q = search.toLowerCase();
      result = result.filter(
        (m) =>
          m.ticker.toLowerCase().includes(q) ||
          (m.title && m.title.toLowerCase().includes(q))
      );
    }
    return [...result].sort((a, b) => {
      let cmp = 0;
      switch (sortKey) {
        case "ticker": cmp = a.ticker.localeCompare(b.ticker); break;
        case "title": {
          const sa = a.ticker.match(/-T(\d+(?:\.\d+)?)$/)?.[1] ?? "0";
          const sb = b.ticker.match(/-T(\d+(?:\.\d+)?)$/)?.[1] ?? "0";
          cmp = Number(sa) - Number(sb);
          break;
        }
        case "yes_bid": cmp = (a.yes_bid ?? 0) - (b.yes_bid ?? 0); break;
        case "yes_ask": cmp = (a.yes_ask ?? 0) - (b.yes_ask ?? 0); break;
        case "last": cmp = (a.last ?? 0) - (b.last ?? 0); break;
        case "volume": cmp = (a.volume ?? 0) - (b.volume ?? 0); break;
        case "close_time": cmp = (a.close_time ?? "").localeCompare(b.close_time ?? ""); break;
      }
      return sortDir === "asc" ? cmp : -cmp;
    });
  }, [markets, search, sortKey, sortDir]);

  function handleSort(key: SortKey) {
    if (sortKey === key) {
      setSortDir(sortDir === "asc" ? "desc" : "asc");
    } else {
      setSortKey(key);
      setSortDir(key === "volume" ? "desc" : "asc");
    }
  }

  const fmtPrice = (v: number | null) => {
    if (v == null) return "-";
    return `$${v.toFixed(2)}`;
  };
  const fmtSpread = (bid: number | null, ask: number | null) => {
    if (bid == null || ask == null) return "-";
    return `$${(ask - bid).toFixed(2)}`;
  };
  const fmtInt = (v: number | null) => v != null ? v.toLocaleString() : "-";
  const fmtTime = (v: string | null) =>
    v ? new Date(v).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) : "-";
  /** Extract strike price from ticker (e.g. "KXBTCD-26MAR0617-T67749.99" → "$67,749.99") */
  const fmtStrike = (ticker: string) => {
    const m = ticker.match(/-T(\d+(?:\.\d+)?)$/);
    if (!m) return ticker;
    return "$" + Number(m[1]).toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-xl font-bold">Markets</h1>
        </div>
        <span className="text-xs text-fg-muted">
          {filtered ? `${filtered.length} markets` : event ? "Loading..." : "Select an event"}
        </span>
      </div>

      {/* Cascading filters */}
      <div className="flex items-center gap-3 flex-wrap">
        <select
          value={exchange}
          onChange={(e) => handleExchangeChange(e.target.value)}
          className="rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm text-fg focus:border-accent focus:outline-none"
        >
          <option value="">All Exchanges</option>
          <option value="kalshi">Kalshi</option>
          <option value="kraken">Kraken</option>
          <option value="polymarket">Polymarket</option>
        </select>

        <select
          value={category ?? ""}
          onChange={(e) => handleCategoryChange(e.target.value)}
          className="rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm text-fg focus:border-accent focus:outline-none"
        >
          <option value="">Select Category</option>
          {filteredCategories?.map((c) => (
            <option key={c.name} value={c.name}>
              {c.name} ({categoryCount(c)})
            </option>
          ))}
        </select>

        <select
          value={series ?? ""}
          onChange={(e) => handleSeriesChange(e.target.value)}
          disabled={!category}
          className="rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm text-fg focus:border-accent focus:outline-none disabled:opacity-50"
        >
          <option value="">Select Series</option>
          {filteredSeries?.map((s) => (
            <option key={s.ticker} value={s.ticker}>
              {s.ticker} — {s.title} ({seriesCount(s)})
            </option>
          ))}
        </select>

        <select
          value={event ?? ""}
          onChange={(e) => handleEventChange(e.target.value)}
          disabled={!series}
          className="rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm text-fg focus:border-accent focus:outline-none disabled:opacity-50"
        >
          <option value="">Select Event</option>
          {events?.map((ev) => (
            <option key={ev.ticker} value={ev.ticker}>
              {ev.title} ({eventCount(ev)})
            </option>
          ))}
        </select>

        {event && (
          <input
            type="text"
            placeholder="Search ticker or title..."
            value={search}
            onChange={(e) => handleSearchChange(e.target.value)}
            className="rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none w-64"
          />
        )}
      </div>

      {error && (
        <p className="text-sm text-red">Error loading markets: {error.message}</p>
      )}

      {/* Market table — only shown when event is selected */}
      {event && (
        <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-xs text-fg-muted border-b border-border">
                  <SortTh k="ticker" current={sortKey} dir={sortDir} onClick={handleSort}>Ticker</SortTh>
                  <SortTh k="title" current={sortKey} dir={sortDir} onClick={handleSort}>Strike</SortTh>
                  <th className="px-4 py-2">Status</th>
                  <SortTh k="yes_bid" current={sortKey} dir={sortDir} onClick={handleSort} align="right">Bid</SortTh>
                  <SortTh k="yes_ask" current={sortKey} dir={sortDir} onClick={handleSort} align="right">Ask</SortTh>
                  <th className="px-4 py-2 text-right">Spread</th>
                  <SortTh k="last" current={sortKey} dir={sortDir} onClick={handleSort} align="right">Last</SortTh>
                  <SortTh k="volume" current={sortKey} dir={sortDir} onClick={handleSort} align="right">Volume</SortTh>
                  <th className="px-4 py-2 text-right">OI</th>
                  <SortTh k="close_time" current={sortKey} dir={sortDir} onClick={handleSort}>Close</SortTh>
                </tr>
              </thead>
              <tbody>
                {filtered && filtered.length > 0 ? (
                  filtered.map((m) => (
                    <tr key={m.ticker} className="border-b border-border-subtle hover:bg-bg-surface-hover">
                      <td className="px-4 py-2 font-mono text-xs">{m.ticker}</td>
                      <td className="px-4 py-2 font-mono" title={m.title ?? undefined}>{fmtStrike(m.ticker)}</td>
                      <td className="px-4 py-2"><MarketStatusBadge status={m.status} /></td>
                      <td className="px-4 py-2 font-mono text-right">{fmtPrice(m.yes_bid)}</td>
                      <td className="px-4 py-2 font-mono text-right">{fmtPrice(m.yes_ask)}</td>
                      <td className="px-4 py-2 font-mono text-right text-fg-muted">{fmtSpread(m.yes_bid, m.yes_ask)}</td>
                      <td className="px-4 py-2 font-mono text-right">{fmtPrice(m.last)}</td>
                      <td className="px-4 py-2 font-mono text-right">{fmtInt(m.volume)}</td>
                      <td className="px-4 py-2 font-mono text-right">{fmtInt(m.open_interest)}</td>
                      <td className="px-4 py-2 text-xs text-fg-muted font-mono">{fmtTime(m.close_time)}</td>
                    </tr>
                  ))
                ) : (
                  <tr>
                    <td colSpan={10} className="px-4 py-8 text-center text-fg-subtle text-sm">
                      {filtered ? "No markets match filters" : "Loading..."}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Prompt when no event selected */}
      {!event && !error && (
        <div className="bg-bg-raised border border-border rounded-lg p-8 text-center text-fg-subtle">
          <p className="text-sm">Select a category, series, and event above to view live market prices.</p>
        </div>
      )}
    </div>
  );
}

function SortTh({ k, current, dir, onClick, align, children }: {
  k: SortKey; current: SortKey; dir: SortDir; onClick: (k: SortKey) => void;
  align?: "right"; children: React.ReactNode;
}) {
  const active = current === k;
  const arrow = active ? (dir === "asc" ? " \u25B2" : " \u25BC") : "";
  return (
    <th
      className={`px-4 py-2 cursor-pointer select-none hover:text-fg transition-colors ${align === "right" ? "text-right" : ""} ${active ? "text-fg" : ""}`}
      onClick={() => onClick(k)}
    >
      {children}{arrow}
    </th>
  );
}

const marketStatusStyles: Record<string, string> = {
  active: "bg-green/15 text-green",
  suspended: "bg-yellow/15 text-yellow",
  closed: "bg-fg-subtle/15 text-fg-subtle",
  settled: "bg-emerald/15 text-emerald",
  inactive: "bg-slate/15 text-slate",
  determined: "bg-blue-light/15 text-blue-light",
};

function MarketStatusBadge({ status }: { status: string }) {
  const style = marketStatusStyles[status] ?? "bg-fg-subtle/15 text-fg-subtle";
  return (
    <span className={`inline-block rounded-md px-2 py-0.5 text-xs font-medium font-mono ${style}`}>
      {status}
    </span>
  );
}
