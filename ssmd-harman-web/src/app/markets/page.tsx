"use client";

import { Suspense, useState, useMemo, useCallback } from "react";
import { useSearchParams, useRouter } from "next/navigation";
import { useCategories, useSeries, useEvents, useMarkets, useMarketSearch, useInfo, usePositions, useWatchlist, useWatchlistData } from "@/lib/hooks";
import type { MonitorCategory, MonitorSeries, MonitorEvent, MonitorMarket, WatchlistItem, WatchlistResult } from "@/lib/types";
import { MarketSlideOver } from "@/components/market-slide-over";

type SortKey = "ticker" | "title" | "yes_bid" | "yes_ask" | "last" | "volume" | "close_time";
type SortDir = "asc" | "desc";
type Exchange = "" | "kalshi" | "kraken" | "polymarket";

const EXCHANGE_LABELS: Record<string, string> = {
  kalshi: "Kalshi",
  kraken: "Kraken",
  "kraken-futures": "Kraken",
  polymarket: "Polymarket",
};

/** Detect exchange from field names present on monitor objects. */
function categoryExchange(cat: MonitorCategory): Exchange | null {
  const hasKalshi = cat.event_count != null || cat.series_count != null;
  const hasKraken = cat.base_count != null || cat.instrument_count != null;
  const hasPM = cat.pm_condition_count != null;
  if (hasKraken) return "kraken";
  if (hasKalshi && hasPM) return null;
  if (hasPM) return "polymarket";
  if (hasKalshi) return "kalshi";
  return null;
}

function seriesExchange(s: MonitorSeries): Exchange {
  if (s.active_pairs != null) return "kraken";
  if (s.active_conditions != null) return "polymarket";
  return "kalshi";
}

function categoryCount(c: MonitorCategory, exchange: Exchange): string {
  const parts: string[] = [];
  if (!exchange || exchange === "kalshi") {
    if (c.event_count != null) parts.push(`${c.event_count} events`);
  }
  if (!exchange || exchange === "kraken") {
    if (c.instrument_count != null) parts.push(`${c.instrument_count} instruments`);
  }
  if (!exchange || exchange === "polymarket") {
    if (c.pm_condition_count != null) parts.push(`${c.pm_condition_count} conditions`);
  }
  return parts.join(", ") || "";
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

/** Staleness dot: green < 5min, yellow < 15min, red otherwise */
function StalenessDot({ snapAt }: { snapAt: number | null }) {
  if (snapAt == null) return <span className="text-red" title="No data">●</span>;
  const age = Date.now() - snapAt;
  if (age < 5 * 60_000) return <span className="text-green" title="Fresh">●</span>;
  if (age < 15 * 60_000) return <span className="text-yellow" title="Stale">●</span>;
  return <span className="text-red" title="Very stale">●</span>;
}

/** Convert a WatchlistResult to MonitorMarket shape for slide-over */
function watchlistToMarket(r: WatchlistResult, title?: string): MonitorMarket {
  return {
    ticker: r.ticker,
    title: title || r.ticker,
    status: "active",
    close_time: null,
    yes_bid: r.yes_bid,
    yes_ask: r.yes_ask,
    last: r.last,
    volume: r.volume,
    open_interest: r.open_interest,
    exchange: r.exchange,
  };
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
  const { data: info } = useInfo();

  // Read filter state from URL params
  const exchange = (searchParams.get("exchange") ?? "") as Exchange;
  const category = searchParams.get("category");
  const series = searchParams.get("series");
  const event = searchParams.get("event");
  const search = searchParams.get("q") ?? "";
  const [sortKey, setSortKey] = useState<SortKey>("ticker");
  const [sortDir, setSortDir] = useState<SortDir>("asc");
  const [slideOverMarket, setSlideOverMarket] = useState<MonitorMarket | null>(null);

  // Watchlist
  const watchlist = useWatchlist();
  const { data: watchlistData } = useWatchlistData(watchlist.items);

  // Auto-set exchange from instance info on first load
  const effectiveExchange = exchange || (info?.exchange as Exchange) || "";

  const { data: categories } = useCategories();
  const { data: seriesList } = useSeries(category);
  const { data: events } = useEvents(series);
  const { data: markets, error } = useMarkets(event);
  const { data: searchResults } = useMarketSearch(search && !event ? search : null, effectiveExchange || undefined);
  const { data: positions } = usePositions();

  // Build position set for overlay
  const positionTickers = useMemo(() => {
    if (!positions) return new Set<string>();
    const set = new Set<string>();
    for (const p of positions.exchange) set.add(p.ticker);
    for (const p of positions.local) set.add(p.ticker);
    return set;
  }, [positions]);

  const filteredCategories = useMemo(() => {
    if (!categories) return undefined;
    if (!effectiveExchange) return categories;
    return categories.filter((c) => {
      const ex = categoryExchange(c);
      if (ex === null) return true;
      if (ex === effectiveExchange) return true;
      return false;
    });
  }, [categories, effectiveExchange]);

  const filteredSeries = useMemo(() => {
    if (!seriesList) return undefined;
    if (!effectiveExchange) return seriesList;
    return seriesList.filter((s) => seriesExchange(s) === effectiveExchange);
  }, [seriesList, effectiveExchange]);

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

  // Filter + sort markets (from cascade or search)
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
  const fmtStrike = (ticker: string) => {
    const m = ticker.match(/-T(\d+(?:\.\d+)?)$/);
    if (!m) return ticker;
    return "$" + Number(m[1]).toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  };
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const fmtTicker = (m: any) => {
    if (m.exchange === "polymarket") return m.outcome ?? m.ticker;
    if (m.exchange === "kraken-futures") {
      const t: string = m.ticker;
      return t.startsWith("kraken:") ? t.slice(7) : t;
    }
    return m.ticker;
  };
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const fmtTitle = (m: any) => {
    if (m.exchange === "polymarket") return m.price ?? "-";
    if (m.exchange === "kraken-futures") return m.mark_price ? `$${Number(m.mark_price).toLocaleString()}` : "-";
    return fmtStrike(m.ticker);
  };

  // Toggle star for search result or cascade market
  const toggleStar = (ticker: string, exchangeName: string, title?: string) => {
    if (watchlist.has(ticker)) {
      watchlist.remove(ticker);
    } else {
      watchlist.add({ ticker, exchange: exchangeName, title });
    }
  };

  // Group watchlist items by exchange for display
  const watchlistByExchange = useMemo(() => {
    const groups: Record<string, { item: WatchlistItem; result?: WatchlistResult }[]> = {};
    for (const item of watchlist.items) {
      const ex = item.exchange;
      if (!groups[ex]) groups[ex] = [];
      const result = watchlistData?.results.find((r) => r.ticker === item.ticker);
      groups[ex].push({ item, result });
    }
    return groups;
  }, [watchlist.items, watchlistData]);

  // Show search results when searching without a cascade selection
  const showSearchResults = search && !event && searchResults?.results && searchResults.results.length > 0;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold">Markets</h1>
        <span className="text-xs text-fg-muted">
          {filtered ? `${filtered.length} markets` : event ? "Loading..." : showSearchResults ? `${searchResults.results.length} results` : "Search or browse"}
        </span>
      </div>

      {/* Search bar — always visible */}
      <input
        type="text"
        placeholder="Search markets by ticker or title..."
        value={search}
        onChange={(e) => handleSearchChange(e.target.value)}
        className="w-full rounded-md border border-border bg-bg-surface px-4 py-2 text-sm text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none"
      />

      {/* Watchlist panel — between search and cascade */}
      {watchlist.items.length > 0 && (
        <details open>
          <summary className="text-xs text-fg-muted cursor-pointer hover:text-fg select-none flex items-center gap-2">
            <span>Watchlist ({watchlist.items.length})</span>
            <button
              onClick={(e) => { e.preventDefault(); watchlist.clear(); }}
              className="text-xs text-red hover:text-red/80 ml-auto"
            >
              Clear all
            </button>
          </summary>
          <div className="mt-3 bg-bg-raised border border-border rounded-lg overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-xs text-fg-muted border-b border-border">
                    <th className="px-4 py-2">Ticker</th>
                    <th className="px-4 py-2">Title</th>
                    <th className="px-4 py-2 text-right">Bid</th>
                    <th className="px-4 py-2 text-right">Ask</th>
                    <th className="px-4 py-2 text-right">Last</th>
                    <th className="px-4 py-2 text-right">Volume</th>
                    <th className="px-4 py-2 text-right">OI</th>
                    <th className="px-4 py-2 w-4"></th>
                    <th className="px-4 py-2 w-6"></th>
                  </tr>
                </thead>
                <tbody>
                  {Object.entries(watchlistByExchange).map(([ex, entries]) => (
                    <WatchlistExchangeGroup
                      key={ex}
                      exchange={ex}
                      entries={entries}
                      fmtPrice={fmtPrice}
                      fmtInt={fmtInt}
                      onRemove={watchlist.remove}
                      onRowClick={(r, title) => setSlideOverMarket(watchlistToMarket(r, title))}
                    />
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </details>
      )}

      {/* Cascade filters — collapsible */}
      <details className="group">
        <summary className="text-xs text-fg-muted cursor-pointer hover:text-fg select-none">
          Browse by category {category ? `— ${category}` : ""}
          {series ? ` / ${series}` : ""}
          {event ? ` / ${event}` : ""}
        </summary>
        <div className="flex items-center gap-3 flex-wrap mt-3">
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
                {c.name} ({categoryCount(c, effectiveExchange)})
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
        </div>
      </details>

      {error && (
        <p className="text-sm text-red">Error loading markets: {error.message}</p>
      )}

      {/* Search results (when searching without cascade) */}
      {showSearchResults && (
        <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-xs text-fg-muted border-b border-border">
                  <th className="px-4 py-2 w-6"></th>
                  <th className="px-4 py-2">Ticker</th>
                  <th className="px-4 py-2">Title</th>
                  <th className="px-4 py-2">Status</th>
                  <th className="px-4 py-2 text-right">Bid</th>
                  <th className="px-4 py-2 text-right">Ask</th>
                  <th className="px-4 py-2 text-right">Last</th>
                  <th className="px-4 py-2 text-right">Volume</th>
                  <th className="px-4 py-2 text-right">OI</th>
                  <th className="px-4 py-2 w-6"></th>
                </tr>
              </thead>
              <tbody>
                {searchResults.results.map((r) => (
                  <tr key={r.ticker} className="border-b border-border-subtle hover:bg-bg-surface-hover cursor-pointer"
                    onClick={() => setSlideOverMarket(r as MonitorMarket)}>
                    <td className="px-4 py-2">
                      <button
                        onClick={(e) => { e.stopPropagation(); toggleStar(r.ticker, r.exchange || "kalshi", r.title); }}
                        className={`text-sm ${watchlist.has(r.ticker) ? "text-yellow" : "text-fg-subtle hover:text-yellow"}`}
                        title={watchlist.has(r.ticker) ? "Remove from watchlist" : "Add to watchlist"}
                      >
                        {watchlist.has(r.ticker) ? "\u2605" : "\u2606"}
                      </button>
                    </td>
                    <td className="px-4 py-2 font-mono text-xs">
                      {r.ticker}
                      {positionTickers.has(r.ticker) && (
                        <span className="ml-1 text-accent text-xs" title="Has position">●</span>
                      )}
                    </td>
                    <td className="px-4 py-2 text-xs text-fg-muted truncate max-w-[200px]">{r.title || "-"}</td>
                    <td className="px-4 py-2"><MarketStatusBadge status={r.status || "-"} /></td>
                    <td className="px-4 py-2 font-mono text-right">{fmtPrice(r.yes_bid ?? null)}</td>
                    <td className="px-4 py-2 font-mono text-right">{fmtPrice(r.yes_ask ?? null)}</td>
                    <td className="px-4 py-2 font-mono text-right">{fmtPrice(r.last ?? null)}</td>
                    <td className="px-4 py-2 font-mono text-right text-xs">{fmtInt(r.volume ?? null)}</td>
                    <td className="px-4 py-2 font-mono text-right text-xs">{fmtInt(r.open_interest ?? null)}</td>
                    <td className="px-4 py-2 text-fg-muted">&rarr;</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Market table — cascade view */}
      {event && (
        <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-xs text-fg-muted border-b border-border">
                  <th className="px-4 py-2 w-6"></th>
                  <SortTh k="ticker" current={sortKey} dir={sortDir} onClick={handleSort}>Ticker</SortTh>
                  <SortTh k="title" current={sortKey} dir={sortDir} onClick={handleSort}>Strike</SortTh>
                  <th className="px-4 py-2">Status</th>
                  <SortTh k="yes_bid" current={sortKey} dir={sortDir} onClick={handleSort} align="right">Bid</SortTh>
                  <SortTh k="yes_ask" current={sortKey} dir={sortDir} onClick={handleSort} align="right">Ask</SortTh>
                  <th className="px-4 py-2 text-right">Spread</th>
                  <SortTh k="last" current={sortKey} dir={sortDir} onClick={handleSort} align="right">Last</SortTh>
                  <SortTh k="volume" current={sortKey} dir={sortDir} onClick={handleSort} align="right">Volume</SortTh>
                  <th className="px-4 py-2 text-right">OI</th>
                  <th className="px-4 py-2 w-4"></th>
                  <SortTh k="close_time" current={sortKey} dir={sortDir} onClick={handleSort}>Close</SortTh>
                </tr>
              </thead>
              <tbody>
                {filtered && filtered.length > 0 ? (
                  filtered.map((m) => (
                    <tr key={m.ticker}
                      className="border-b border-border-subtle hover:bg-bg-surface-hover cursor-pointer"
                      onClick={() => setSlideOverMarket(m)}>
                      <td className="px-4 py-2">
                        <button
                          onClick={(e) => { e.stopPropagation(); toggleStar(m.ticker, m.exchange || "kalshi", m.title); }}
                          className={`text-sm ${watchlist.has(m.ticker) ? "text-yellow" : "text-fg-subtle hover:text-yellow"}`}
                          title={watchlist.has(m.ticker) ? "Remove from watchlist" : "Add to watchlist"}
                        >
                          {watchlist.has(m.ticker) ? "\u2605" : "\u2606"}
                        </button>
                      </td>
                      <td className="px-4 py-2 font-mono text-xs">
                        {fmtTicker(m)}
                        {positionTickers.has(m.ticker) && (
                          <span className="ml-1 text-accent text-xs" title="Has position">●</span>
                        )}
                      </td>
                      <td className="px-4 py-2 font-mono" title={m.title ?? undefined}>{fmtTitle(m)}</td>
                      <td className="px-4 py-2"><MarketStatusBadge status={m.status} /></td>
                      <td className="px-4 py-2 font-mono text-right">{fmtPrice(m.yes_bid ?? m.bid ?? m.best_bid ?? null)}</td>
                      <td className="px-4 py-2 font-mono text-right">{fmtPrice(m.yes_ask ?? m.ask ?? m.best_ask ?? null)}</td>
                      <td className="px-4 py-2 font-mono text-right text-fg-muted">{fmtSpread(m.yes_bid ?? m.bid ?? m.best_bid ?? null, m.yes_ask ?? m.ask ?? m.best_ask ?? null)}</td>
                      <td className="px-4 py-2 font-mono text-right">{fmtPrice(m.last ?? (m.price != null ? Number(m.price) : null))}</td>
                      <td className="px-4 py-2 font-mono text-right">{fmtInt(m.volume)}</td>
                      <td className="px-4 py-2 font-mono text-right">{fmtInt(m.open_interest)}</td>
                      <td className="px-4 py-2 text-fg-muted">&rarr;</td>
                      <td className="px-4 py-2 text-xs text-fg-muted font-mono">{fmtTime(m.close_time)}</td>
                    </tr>
                  ))
                ) : (
                  <tr>
                    <td colSpan={12} className="px-4 py-8 text-center text-fg-subtle text-sm">
                      {filtered ? "No markets match filters" : "Loading..."}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Prompt when nothing selected */}
      {!event && !error && !showSearchResults && watchlist.items.length === 0 && (
        <div className="bg-bg-raised border border-border rounded-lg p-8 text-center text-fg-subtle">
          <p className="text-sm">Search above or browse categories to view live market prices.</p>
        </div>
      )}

      {/* Slide-over panel */}
      {slideOverMarket && (
        <MarketSlideOver
          market={slideOverMarket}
          onClose={() => setSlideOverMarket(null)}
        />
      )}
    </div>
  );
}

/** Watchlist rows grouped under an exchange header */
function WatchlistExchangeGroup({
  exchange,
  entries,
  fmtPrice,
  fmtInt,
  onRemove,
  onRowClick,
}: {
  exchange: string;
  entries: { item: WatchlistItem; result?: WatchlistResult }[];
  fmtPrice: (v: number | null) => string;
  fmtInt: (v: number | null) => string;
  onRemove: (ticker: string) => void;
  onRowClick: (r: WatchlistResult, title?: string) => void;
}) {
  return (
    <>
      <tr className="bg-bg-surface">
        <td colSpan={9} className="px-4 py-1 text-xs font-medium text-fg-muted">
          {EXCHANGE_LABELS[exchange] || exchange}
        </td>
      </tr>
      {entries.map(({ item, result }) => (
        <tr
          key={item.ticker}
          className="border-b border-border-subtle hover:bg-bg-surface-hover cursor-pointer"
          onClick={() => result && onRowClick(result, item.title)}
        >
          <td className="px-4 py-2 font-mono text-xs">{item.ticker}</td>
          <td className="px-4 py-2 text-xs text-fg-muted truncate max-w-[200px]">{item.title || "-"}</td>
          <td className="px-4 py-2 font-mono text-right">{fmtPrice(result?.yes_bid ?? null)}</td>
          <td className="px-4 py-2 font-mono text-right">{fmtPrice(result?.yes_ask ?? null)}</td>
          <td className="px-4 py-2 font-mono text-right">{fmtPrice(result?.last ?? null)}</td>
          <td className="px-4 py-2 font-mono text-right text-xs">{fmtInt(result?.volume ?? null)}</td>
          <td className="px-4 py-2 font-mono text-right text-xs">{fmtInt(result?.open_interest ?? null)}</td>
          <td className="px-4 py-2 text-center"><StalenessDot snapAt={result?.snap_at ?? null} /></td>
          <td className="px-4 py-2">
            <button
              onClick={(e) => { e.stopPropagation(); onRemove(item.ticker); }}
              className="text-fg-subtle hover:text-red text-sm"
              title="Remove from watchlist"
            >
              &times;
            </button>
          </td>
        </tr>
      ))}
    </>
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
