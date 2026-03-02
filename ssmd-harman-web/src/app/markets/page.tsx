"use client";

import { Suspense, useState, useMemo, useCallback, useEffect, useRef } from "react";
import { useSearchParams, useRouter } from "next/navigation";
import { useSeriesSearch, useOutcomeSearch, useInfo, usePositions, useWatchlist, useWatchlistData } from "@/lib/hooks";
import type { MonitorMarket, MonitorSearchResult, WatchlistItem, WatchlistResult } from "@/lib/types";
import { MarketSlideOver } from "@/components/market-slide-over";

type Exchange = "" | "kalshi" | "kraken" | "polymarket";

const EXCHANGE_LABELS: Record<string, string> = {
  kalshi: "Kalshi",
  kraken: "Kraken",
  "kraken-futures": "Kraken",
  polymarket: "Polymarket",
};

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

  const exchange = (searchParams.get("exchange") ?? "") as Exchange;
  const search = searchParams.get("q") ?? "";
  const searchRef = useRef<HTMLInputElement>(null);
  const [slideOverMarket, setSlideOverMarket] = useState<MonitorMarket | null>(null);

  // Cmd+P focuses search input
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "p") {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  // Watchlist
  const watchlist = useWatchlist();
  const { data: watchlistData } = useWatchlistData(watchlist.items);

  // Two separate search hooks
  const { data: seriesResults } = useSeriesSearch(search || null, exchange || undefined);
  const { data: outcomeResults } = useOutcomeSearch(search || null, exchange || undefined);
  const { data: positions } = usePositions();

  // Build position set for overlay
  const positionTickers = useMemo(() => {
    if (!positions) return new Set<string>();
    const set = new Set<string>();
    for (const p of positions.exchange) set.add(p.ticker);
    for (const p of positions.local) set.add(p.ticker);
    return set;
  }, [positions]);

  const setParams = useCallback((updates: Record<string, string | null>) => {
    const params = new URLSearchParams(searchParams.toString());
    for (const [k, v] of Object.entries(updates)) {
      if (v) params.set(k, v);
      else params.delete(k);
    }
    router.replace(`/markets?${params.toString()}`);
  }, [searchParams, router]);

  const handleSearchChange = (val: string) => {
    setParams({ q: val || null });
  };

  const fmtPrice = (v: number | null) => {
    if (v == null) return "-";
    return `$${v.toFixed(2)}`;
  };
  const fmtInt = (v: number | null) => v != null ? v.toLocaleString() : "-";

  // Toggle star for search result
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

  const hasSeries = seriesResults?.results && seriesResults.results.length > 0;
  const hasOutcomes = outcomeResults?.results && outcomeResults.results.length > 0;
  const hasSearch = search.length >= 2;
  const totalResults = (seriesResults?.results?.length ?? 0) + (outcomeResults?.results?.length ?? 0);

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold">Markets</h1>
        <span className="text-xs text-fg-muted">
          {hasSearch ? `${totalResults} results` : "Search markets"}
        </span>
      </div>

      {/* Search bar + exchange filter */}
      <div className="flex gap-2">
        <select
          value={exchange}
          onChange={(e) => setParams({ exchange: e.target.value || null })}
          className="rounded-md border border-border bg-bg-surface px-3 py-2 text-sm text-fg focus:border-accent focus:outline-none shrink-0"
        >
          <option value="">All</option>
          <option value="kalshi">Kalshi</option>
          <option value="kraken">Kraken</option>
          <option value="polymarket">Polymarket</option>
        </select>
        <input
          ref={searchRef}
          type="text"
          placeholder="Search series or outcomes... (⌘P)"
          value={search}
          onChange={(e) => handleSearchChange(e.target.value)}
          className="w-full rounded-md border border-border bg-bg-surface px-4 py-2 text-sm text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none"
        />
      </div>

      {/* Watchlist panel */}
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

      {/* Series results */}
      {hasSearch && hasSeries && (
        <SearchResultsSection
          title="Series"
          results={seriesResults.results}
          positionTickers={positionTickers}
          watchlist={watchlist}
          toggleStar={toggleStar}
          fmtPrice={fmtPrice}
          fmtInt={fmtInt}
          onRowClick={(r) => setSlideOverMarket(r as MonitorMarket)}
        />
      )}

      {/* Outcome results */}
      {hasSearch && hasOutcomes && (
        <SearchResultsSection
          title="Outcomes"
          results={outcomeResults.results}
          positionTickers={positionTickers}
          watchlist={watchlist}
          toggleStar={toggleStar}
          fmtPrice={fmtPrice}
          fmtInt={fmtInt}
          onRowClick={(r) => setSlideOverMarket(r as MonitorMarket)}
        />
      )}

      {/* No results */}
      {hasSearch && !hasSeries && !hasOutcomes && (
        <div className="bg-bg-raised border border-border rounded-lg p-8 text-center text-fg-subtle">
          <p className="text-sm">No results for &ldquo;{search}&rdquo;</p>
        </div>
      )}

      {/* Prompt when nothing searched */}
      {!hasSearch && watchlist.items.length === 0 && (
        <div className="bg-bg-raised border border-border rounded-lg p-8 text-center text-fg-subtle">
          <p className="text-sm">Search above to find series or market outcomes.</p>
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

/** Reusable search results table */
function SearchResultsSection({
  title,
  results,
  positionTickers,
  watchlist,
  toggleStar,
  fmtPrice,
  fmtInt,
  onRowClick,
}: {
  title: string;
  results: MonitorSearchResult[];
  positionTickers: Set<string>;
  watchlist: { has: (ticker: string) => boolean };
  toggleStar: (ticker: string, exchange: string, title?: string) => void;
  fmtPrice: (v: number | null) => string;
  fmtInt: (v: number | null) => string;
  onRowClick: (r: MonitorSearchResult) => void;
}) {
  return (
    <div>
      <h2 className="text-sm font-medium text-fg-muted mb-2">{title} ({results.length})</h2>
      <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-fg-muted border-b border-border">
                <th className="px-4 py-2 w-6"></th>
                <th className="px-4 py-2">Ticker</th>
                <th className="px-4 py-2">Title</th>
                <th className="px-4 py-2">Exchange</th>
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
              {results.map((r) => (
                <tr key={r.ticker} className="border-b border-border-subtle hover:bg-bg-surface-hover cursor-pointer"
                  onClick={() => onRowClick(r)}>
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
                  <td className="px-4 py-2 text-xs text-fg-muted">{EXCHANGE_LABELS[r.exchange || ""] || r.exchange || "-"}</td>
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
