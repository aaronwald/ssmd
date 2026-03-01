"use client";

import { useState, useEffect, useCallback } from "react";
import useSWR from "swr";
import {
  getHealth,
  listOrders,
  listGroups,
  listFills,
  listAudit,
  getPositions,
  getRisk,
  getSnapMap,
  getCategories,
  getSeries,
  getEvents,
  getMarkets,
  searchMonitorMarkets,
  getInfo,
  getMe,
  getAdminUsers,
  getApiInstance,
  fetchWatchlist,
} from "./api";
import type { WatchlistItem } from "./types";

const REFRESH_INTERVAL = 2500;
const METADATA_REFRESH = 60000; // 60s for metadata (categories, series, events)
const LIVE_REFRESH = 2500; // 2.5s for live prices (markets)

/** Prefix SWR key with current instance. Returns null (pauses SWR) when no instance selected. */
function instanceKey(key: string): string | null {
  const inst = getApiInstance();
  return inst ? `${inst}:${key}` : null;
}

/** SWR mutate matcher: matches any instance-prefixed key ending with `:suffix` (or `:suffix-*`). */
export function matchInstanceKey(suffix: string): (key: string) => boolean {
  return (key: string) => typeof key === "string" && key.includes(`:${suffix}`);
}

export function useHealth() {
  return useSWR(instanceKey("health"), getHealth, {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function useOrders(state?: string) {
  const key = state ? `orders-${state}` : "orders";
  return useSWR(instanceKey(key), () => listOrders(state), {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function useGroups(state?: string) {
  const key = state ? `groups-${state}` : "groups";
  return useSWR(instanceKey(key), () => listGroups(state), {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function useFills() {
  return useSWR(instanceKey("fills"), listFills, {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function useAudit() {
  return useSWR(instanceKey("audit"), listAudit, {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function usePositions() {
  return useSWR(instanceKey("positions"), getPositions, {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function useRisk() {
  return useSWR(instanceKey("risk"), getRisk, {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function useSnapMap(feed: string = "kalshi") {
  return useSWR(instanceKey(`snap-${feed}`), () => getSnapMap(feed), {
    refreshInterval: REFRESH_INTERVAL,
  });
}

// Monitor hierarchy hooks — global market data (not instance-scoped)
export function useCategories() {
  return useSWR("data-categories", getCategories, {
    refreshInterval: METADATA_REFRESH,
  });
}

export function useSeries(category: string | null) {
  return useSWR(
    category ? `data-series-${category}` : null,
    () => getSeries(category!),
    { refreshInterval: METADATA_REFRESH }
  );
}

export function useEvents(series: string | null) {
  return useSWR(
    series ? `data-events-${series}` : null,
    () => getEvents(series!),
    { refreshInterval: METADATA_REFRESH }
  );
}

export function useMarkets(event: string | null) {
  return useSWR(
    event ? `data-markets-${event}` : null,
    () => getMarkets(event!),
    { refreshInterval: LIVE_REFRESH }
  );
}

export function useInfo() {
  return useSWR(instanceKey("info"), getInfo, { revalidateOnFocus: false });
}

export function useMe() {
  return useSWR(instanceKey("me"), getMe, { revalidateOnFocus: false });
}

export function useMarketSearch(q: string | null, exchange?: string) {
  return useSWR(
    q && q.length >= 2 ? `data-search-${q}-${exchange ?? ""}` : null,
    () => searchMonitorMarkets(q!, exchange),
    { refreshInterval: LIVE_REFRESH, dedupingInterval: 500 }
  );
}

export function useAdminUsers() {
  return useSWR(instanceKey("admin-users"), getAdminUsers, {
    refreshInterval: METADATA_REFRESH,
  });
}

// Watchlist persistence (localStorage)
const WATCHLIST_KEY = "harman-watchlist";

export function useWatchlist() {
  const [items, setItems] = useState<WatchlistItem[]>([]);

  // Load from localStorage after mount (SSR-safe)
  useEffect(() => {
    try {
      const raw = localStorage.getItem(WATCHLIST_KEY);
      if (raw) setItems(JSON.parse(raw));
    } catch { /* ignore corrupt data */ }
  }, []);

  const persist = useCallback((next: WatchlistItem[]) => {
    setItems(next);
    try { localStorage.setItem(WATCHLIST_KEY, JSON.stringify(next)); } catch { /* quota */ }
  }, []);

  const add = useCallback((item: WatchlistItem) => {
    setItems((prev) => {
      if (prev.some((i) => i.ticker === item.ticker)) return prev;
      const next = [...prev, item];
      try { localStorage.setItem(WATCHLIST_KEY, JSON.stringify(next)); } catch { /* quota */ }
      return next;
    });
  }, []);

  const remove = useCallback((ticker: string) => {
    setItems((prev) => {
      const next = prev.filter((i) => i.ticker !== ticker);
      try { localStorage.setItem(WATCHLIST_KEY, JSON.stringify(next)); } catch { /* quota */ }
      return next;
    });
  }, []);

  const has = useCallback((ticker: string) => items.some((i) => i.ticker === ticker), [items]);

  const clear = useCallback(() => persist([]), [persist]);

  return { items, add, remove, has, clear };
}

// Watchlist live data (SWR)
export function useWatchlistData(items: WatchlistItem[]) {
  const key = items.length > 0 ? `data-watchlist-${items.map((i) => i.ticker).join(",")}` : null;
  return useSWR(key, () => fetchWatchlist(items), {
    refreshInterval: LIVE_REFRESH,
    dedupingInterval: 1000,
  });
}
