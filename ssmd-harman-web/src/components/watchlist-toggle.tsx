"use client";

import { useLayout } from "@/lib/layout-context";

export function WatchlistToggle() {
  const { watchlistOpen, toggleWatchlist } = useLayout();
  if (watchlistOpen) return null;

  return (
    <button
      onClick={toggleWatchlist}
      className="fixed right-0 top-1/2 -translate-y-1/2 z-30 bg-bg-raised border border-r-0 border-border rounded-l-md px-1 py-3 text-xs text-fg-muted hover:text-fg"
      title="Open watchlist"
    >
      <span className="[writing-mode:vertical-lr]">Watchlist</span>
    </button>
  );
}
