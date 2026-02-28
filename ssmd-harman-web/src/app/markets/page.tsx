"use client";

import { useState, useMemo } from "react";
import { useCategories, useSeries, useEvents, useMarkets } from "@/lib/hooks";
import type { MonitorMarket } from "@/lib/types";

type SortKey = "ticker" | "title" | "yes_bid" | "yes_ask" | "last" | "volume" | "close_time";
type SortDir = "asc" | "desc";

export default function MarketsPage() {
  const [category, setCategory] = useState<string | null>(null);
  const [series, setSeries] = useState<string | null>(null);
  const [event, setEvent] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("ticker");
  const [sortDir, setSortDir] = useState<SortDir>("asc");

  const { data: categories } = useCategories();
  const { data: seriesList } = useSeries(category);
  const { data: events } = useEvents(series);
  const { data: markets, error } = useMarkets(event);

  // Reset downstream selections when parent changes
  const handleCategoryChange = (val: string) => {
    setCategory(val || null);
    setSeries(null);
    setEvent(null);
  };

  const handleSeriesChange = (val: string) => {
    setSeries(val || null);
    setEvent(null);
  };

  const handleEventChange = (val: string) => {
    setEvent(val || null);
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
        case "title": cmp = (a.title ?? "").localeCompare(b.title ?? ""); break;
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

  const fmtPrice = (v: number | null) => v != null ? `$${v.toFixed(2)}` : "-";
  const fmtSpread = (bid: number | null, ask: number | null) =>
    bid != null && ask != null ? `$${(ask - bid).toFixed(2)}` : "-";
  const fmtInt = (v: number | null) => v != null ? v.toLocaleString() : "-";
  const fmtTime = (v: string | null) =>
    v ? new Date(v).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) : "-";

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold">Markets</h1>
        <span className="text-xs text-fg-muted">
          {filtered ? `${filtered.length} markets` : event ? "Loading..." : "Select an event"}
        </span>
      </div>

      {/* Cascading filters */}
      <div className="flex items-center gap-3 flex-wrap">
        <select
          value={category ?? ""}
          onChange={(e) => handleCategoryChange(e.target.value)}
          className="rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm text-fg focus:border-accent focus:outline-none"
        >
          <option value="">Select Category</option>
          {categories?.map((c) => (
            <option key={c.name} value={c.name}>
              {c.name} ({c.event_count} events, {c.series_count} series)
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
          {seriesList?.map((s) => (
            <option key={s.ticker} value={s.ticker}>
              {s.ticker} — {s.title} ({s.active_events} events)
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
              {ev.ticker} — {ev.title} ({ev.market_count} markets)
            </option>
          ))}
        </select>

        {event && (
          <input
            type="text"
            placeholder="Search ticker or title..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
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
                  <SortTh k="title" current={sortKey} dir={sortDir} onClick={handleSort}>Title</SortTh>
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
                      <td className="px-4 py-2 max-w-xs truncate" title={m.title ?? undefined}>{m.title ?? "-"}</td>
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
                    <td colSpan={9} className="px-4 py-8 text-center text-fg-subtle text-sm">
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
