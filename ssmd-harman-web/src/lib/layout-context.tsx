"use client";

import { createContext, useContext, useState, useEffect, type ReactNode } from "react";

interface LayoutContextValue {
  navCollapsed: boolean;
  toggleNav: () => void;
  watchlistOpen: boolean;
  toggleWatchlist: () => void;
  setWatchlistOpen: (open: boolean) => void;
}

const LayoutContext = createContext<LayoutContextValue>({
  navCollapsed: false,
  toggleNav: () => {},
  watchlistOpen: true,
  toggleWatchlist: () => {},
  setWatchlistOpen: () => {},
});

export function LayoutProvider({ children }: { children: ReactNode }) {
  const [navCollapsed, setNavCollapsed] = useState(false);
  const [watchlistOpen, setWatchlistOpen] = useState(true);

  // Restore from localStorage after mount
  useEffect(() => {
    try {
      const nav = localStorage.getItem("harman-nav-collapsed");
      if (nav === "true") setNavCollapsed(true);
      const wl = localStorage.getItem("harman-watchlist-open");
      if (wl === "false") setWatchlistOpen(false);
    } catch {}
  }, []);

  const toggleNav = () => {
    setNavCollapsed((prev) => {
      const next = !prev;
      try { localStorage.setItem("harman-nav-collapsed", String(next)); } catch {}
      return next;
    });
  };

  const toggleWatchlist = () => {
    setWatchlistOpen((prev) => {
      const next = !prev;
      try { localStorage.setItem("harman-watchlist-open", String(next)); } catch {}
      return next;
    });
  };

  const setWatchlistOpenPersist = (open: boolean) => {
    setWatchlistOpen(open);
    try { localStorage.setItem("harman-watchlist-open", String(open)); } catch {}
  };

  // Keyboard shortcut: [ to toggle nav
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "[" && !e.metaKey && !e.ctrlKey && !(e.target instanceof HTMLInputElement) && !(e.target instanceof HTMLTextAreaElement) && !(e.target instanceof HTMLSelectElement)) {
        toggleNav();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  return (
    <LayoutContext.Provider value={{ navCollapsed, toggleNav, watchlistOpen, toggleWatchlist, setWatchlistOpen: setWatchlistOpenPersist }}>
      {children}
    </LayoutContext.Provider>
  );
}

export function useLayout() {
  return useContext(LayoutContext);
}
