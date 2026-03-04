"use client";

import { useState, useMemo } from "react";
import Link from "next/link";
import { useLayout } from "@/lib/layout-context";
import { useWatchlist, useWatchlistData } from "@/lib/hooks";
import { useInstance } from "@/lib/instance-context";
import { CreateOrderFormControlled } from "./create-order-form-controlled";
import { SnapAgeDot } from "./snap-age-dot";
import type { WatchlistItem, WatchlistResult } from "@/lib/types";

const EXCHANGE_LABELS: Record<string, string> = {
  kalshi: "Kalshi",
  "kraken-futures": "Kraken",
  polymarket: "Polymarket",
};

interface GroupedWatchlist {
  exchange: string;
  label: string;
  items: Array<WatchlistItem & { data?: WatchlistResult }>;
}

export function WatchlistPanel() {
  const { watchlistOpen, toggleWatchlist } = useLayout();
  const watchlist = useWatchlist();
  const { data: watchlistData } = useWatchlistData(watchlist.items);
  const { instance } = useInstance();
  const [expandedTicker, setExpandedTicker] = useState<string | null>(null);

  if (!watchlistOpen) return null;

  const grouped = useMemo((): GroupedWatchlist[] => {
    const byExchange = new Map<string, Array<WatchlistItem & { data?: WatchlistResult }>>();
    const dataMap = new Map<string, WatchlistResult>();
    if (watchlistData?.results) {
      for (const r of watchlistData.results) {
        dataMap.set(r.ticker, r);
      }
    }
    for (const item of watchlist.items) {
      const key = item.exchange || "unknown";
      if (!byExchange.has(key)) byExchange.set(key, []);
      byExchange.get(key)!.push({ ...item, data: dataMap.get(item.ticker) });
    }
    return Array.from(byExchange.entries()).map(([exchange, items]) => ({
      exchange,
      label: EXCHANGE_LABELS[exchange] || exchange,
      items,
    }));
  }, [watchlist.items, watchlistData]);

  const fmtPrice = (v: number | null | undefined) => v != null ? `$${v.toFixed(2)}` : "—";

  return (
    <aside
      className="shrink-0 border-l border-border bg-bg-raised flex flex-col h-full overflow-hidden"
      style={{ width: "var(--width-watchlist)" }}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-border shrink-0">
        <span className="text-xs font-medium text-fg-muted">
          Watchlist ({watchlist.items.length})
        </span>
        <button
          onClick={toggleWatchlist}
          className="text-fg-subtle hover:text-fg text-sm leading-none px-1"
          title="Close watchlist"
        >
          &times;
        </button>
      </div>

      {/* Scrollable list */}
      <div className="flex-1 overflow-y-auto">
        {watchlist.items.length === 0 ? (
          <div className="p-4 text-center text-xs text-fg-subtle">
            <p>No items in watchlist</p>
            <Link href="/markets" className="text-accent hover:underline mt-1 inline-block">
              Search markets to add
            </Link>
          </div>
        ) : (
          grouped.map((group) => (
            <div key={group.exchange}>
              {/* Exchange header */}
              <div className="px-3 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-fg-subtle bg-bg/50 border-b border-border-subtle">
                {group.label}
              </div>

              {group.items.map((item) => {
                const isExpanded = expandedTicker === item.ticker;
                return (
                  <div key={item.ticker} className="border-b border-border-subtle">
                    {/* Watchlist row */}
                    <div
                      className="group flex items-center gap-2 px-3 py-2 hover:bg-bg-surface-hover cursor-pointer"
                      onClick={() => setExpandedTicker(isExpanded ? null : item.ticker)}
                    >
                      <div className="flex-1 min-w-0">
                        <div className="text-xs font-mono text-fg truncate" title={item.ticker}>
                          {item.title || item.ticker}
                        </div>
                        <div className="text-[10px] text-fg-subtle font-mono truncate">
                          {item.ticker}
                        </div>
                      </div>
                      <div className="text-xs font-mono text-fg-muted whitespace-nowrap">
                        {fmtPrice(item.data?.last)}
                      </div>
                      <SnapAgeDot snapAt={item.data?.snap_at ?? null} />
                      <button
                        onClick={(e) => { e.stopPropagation(); watchlist.remove(item.ticker); }}
                        className="opacity-0 group-hover:opacity-100 text-fg-subtle hover:text-red text-xs leading-none px-0.5 transition-opacity"
                        title="Remove from watchlist"
                      >
                        &times;
                      </button>
                    </div>

                    {/* Expanded: inline order form */}
                    {isExpanded && instance && (
                      <div className="px-3 py-2 bg-bg-surface border-t border-border-subtle">
                        <div className="text-[10px] text-fg-subtle mb-1.5 flex gap-3">
                          <span>Bid: <span className="text-fg font-mono">{fmtPrice(item.data?.yes_bid)}</span></span>
                          <span>Ask: <span className="text-fg font-mono">{fmtPrice(item.data?.yes_ask)}</span></span>
                        </div>
                        <CreateOrderFormControlled
                          ticker={item.ticker}
                          yesBid={item.data?.yes_bid ?? null}
                          yesAsk={item.data?.yes_ask ?? null}
                          last={item.data?.last ?? null}
                          onSuccess={() => setExpandedTicker(null)}
                        />
                      </div>
                    )}
                    {isExpanded && !instance && (
                      <div className="px-3 py-2 bg-bg-surface border-t border-border-subtle text-xs text-fg-subtle">
                        Select an instance to place orders
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          ))
        )}
      </div>
    </aside>
  );
}
