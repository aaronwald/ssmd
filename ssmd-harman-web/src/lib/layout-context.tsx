"use client";

import { createContext, useContext, useState, useEffect, type ReactNode } from "react";

interface LayoutContextValue {
  navCollapsed: boolean;
  toggleNav: () => void;
}

const LayoutContext = createContext<LayoutContextValue>({
  navCollapsed: false,
  toggleNav: () => {},
});

export function LayoutProvider({ children }: { children: ReactNode }) {
  const [navCollapsed, setNavCollapsed] = useState(false);

  // Restore from localStorage after mount
  useEffect(() => {
    try {
      const nav = localStorage.getItem("harman-nav-collapsed");
      if (nav === "true") setNavCollapsed(true);
    } catch {}
  }, []);

  const toggleNav = () => {
    setNavCollapsed((prev) => {
      const next = !prev;
      try { localStorage.setItem("harman-nav-collapsed", String(next)); } catch {}
      return next;
    });
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
    <LayoutContext.Provider value={{ navCollapsed, toggleNav }}>
      {children}
    </LayoutContext.Provider>
  );
}

export function useLayout() {
  return useContext(LayoutContext);
}
