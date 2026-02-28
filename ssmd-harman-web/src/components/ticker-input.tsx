"use client";

import { useState, useEffect, useRef } from "react";
import { searchTickers } from "@/lib/api";

export function TickerInput({
  value,
  onChange,
  className,
}: {
  value: string;
  onChange: (v: string) => void;
  className?: string;
}) {
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [open, setOpen] = useState(false);
  const [highlighted, setHighlighted] = useState(-1);
  const [degraded, setDegraded] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (value.length < 1) {
      setSuggestions([]);
      setDegraded(false);
      return;
    }
    const timer = setTimeout(async () => {
      try {
        const res = await searchTickers(value);
        setSuggestions(res.tickers);
        setDegraded(res.degraded === true);
        setOpen(res.tickers.length > 0 || res.degraded === true);
        setHighlighted(-1);
      } catch {
        setSuggestions([]);
        setDegraded(true);
      }
    }, 150);
    return () => clearTimeout(timer);
  }, [value]);

  // Close dropdown on outside click
  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, []);

  function handleKeyDown(e: React.KeyboardEvent) {
    if (!open || suggestions.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setHighlighted((h) => Math.min(h + 1, suggestions.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setHighlighted((h) => Math.max(h - 1, 0));
    } else if (e.key === "Enter" && highlighted >= 0) {
      e.preventDefault();
      onChange(suggestions[highlighted]);
      setOpen(false);
    } else if (e.key === "Escape") {
      setOpen(false);
    }
  }

  return (
    <div ref={ref} className="relative">
      <input
        placeholder="Ticker"
        value={value}
        onChange={(e) => {
          onChange(e.target.value.toUpperCase());
          setOpen(true);
        }}
        onFocus={() => suggestions.length > 0 && setOpen(true)}
        onKeyDown={handleKeyDown}
        className={
          className ||
          "px-3 py-2 rounded-lg bg-bg-surface border border-border text-sm font-mono focus:border-accent outline-none w-full"
        }
      />
      {open && (degraded || suggestions.length > 0) && (
        <ul className="absolute z-50 mt-1 max-h-60 w-full overflow-y-auto rounded-lg bg-bg-raised border border-border shadow-lg">
          {degraded && (
            <li className="px-3 py-1.5 text-xs text-yellow-500">
              Ticker search unavailable — type exact ticker
            </li>
          )}
          {suggestions.map((t, i) => (
            <li
              key={t}
              className={`px-3 py-1.5 text-sm font-mono cursor-pointer ${
                i === highlighted
                  ? "bg-accent/20 text-accent"
                  : "text-fg hover:bg-bg-surface-hover"
              }`}
              onMouseDown={() => {
                onChange(t);
                setOpen(false);
              }}
              onMouseEnter={() => setHighlighted(i)}
            >
              {t}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
