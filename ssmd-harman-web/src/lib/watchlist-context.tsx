"use client";

import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from "react";
import type { WatchlistItem } from "./types";

const WATCHLIST_KEY = "harman-watchlist";

interface WatchlistContextValue {
  items: WatchlistItem[];
  add: (item: WatchlistItem) => void;
  remove: (ticker: string) => void;
  has: (ticker: string) => boolean;
  clear: () => void;
}

const WatchlistContext = createContext<WatchlistContextValue>({
  items: [],
  add: () => {},
  remove: () => {},
  has: () => false,
  clear: () => {},
});

export function WatchlistProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<WatchlistItem[]>([]);

  useEffect(() => {
    try {
      const raw = localStorage.getItem(WATCHLIST_KEY);
      if (raw) setItems(JSON.parse(raw));
    } catch {}
  }, []);

  const persist = useCallback((next: WatchlistItem[]) => {
    setItems(next);
    try { localStorage.setItem(WATCHLIST_KEY, JSON.stringify(next)); } catch {}
  }, []);

  const add = useCallback((item: WatchlistItem) => {
    setItems((prev) => {
      if (prev.some((i) => i.ticker === item.ticker)) return prev;
      const next = [...prev, item];
      try { localStorage.setItem(WATCHLIST_KEY, JSON.stringify(next)); } catch {}
      return next;
    });
  }, []);

  const remove = useCallback((ticker: string) => {
    setItems((prev) => {
      const next = prev.filter((i) => i.ticker !== ticker);
      try { localStorage.setItem(WATCHLIST_KEY, JSON.stringify(next)); } catch {}
      return next;
    });
  }, []);

  const has = useCallback((ticker: string) => items.some((i) => i.ticker === ticker), [items]);

  const clear = useCallback(() => persist([]), [persist]);

  return (
    <WatchlistContext.Provider value={{ items, add, remove, has, clear }}>
      {children}
    </WatchlistContext.Provider>
  );
}

export function useWatchlist() {
  return useContext(WatchlistContext);
}
