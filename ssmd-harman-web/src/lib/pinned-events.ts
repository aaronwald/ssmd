"use client";

import { useState, useEffect, useCallback } from "react";

const PINNED_KEY = "sports-pinned-events";

export function usePinnedEvents() {
  const [pinned, setPinned] = useState<Set<string>>(new Set());

  useEffect(() => {
    try {
      const raw = localStorage.getItem(PINNED_KEY);
      if (raw) setPinned(new Set(JSON.parse(raw)));
    } catch {}
  }, []);

  const toggle = useCallback(
    (ticker: string) => {
      setPinned((prev) => {
        const next = new Set(prev);
        if (next.has(ticker)) next.delete(ticker);
        else next.add(ticker);
        try {
          localStorage.setItem(PINNED_KEY, JSON.stringify([...next]));
        } catch {}
        return next;
      });
    },
    [],
  );

  const isPinned = useCallback((ticker: string) => pinned.has(ticker), [pinned]);

  return { pinned, toggle, isPinned };
}
