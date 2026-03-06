"use client";

import { useState, useMemo, useCallback, useEffect, useRef } from "react";
import { useSeries, useEvents, useMarkets, usePositions } from "@/lib/hooks";
import type { MonitorSeries, MonitorEvent, MonitorMarket } from "@/lib/types";
import { MarketSlideOver } from "@/components/market-slide-over";

const SERIES_LS_KEY = "crypto-selected-series";

/** Read URL search params directly (avoids useSearchParams Suspense re-trigger) */
function getUrlParam(key: string): string | null {
  if (typeof window === "undefined") return null;
  return new URLSearchParams(window.location.search).get(key);
}

/** Update URL search params without Next.js router (no Suspense trigger) */
function setUrlParams(updates: Record<string, string | null>) {
  if (typeof window === "undefined") return;
  const params = new URLSearchParams(window.location.search);
  for (const [k, v] of Object.entries(updates)) {
    if (v) params.set(k, v);
    else params.delete(k);
  }
  const qs = params.toString();
  const url = qs ? `${window.location.pathname}?${qs}` : window.location.pathname;
  window.history.replaceState(null, "", url);
}

export default function CryptoPage() {
  return <CryptoContent />;
}

// --- Helpers ---

function extractStrike(ticker: string): number {
  const match = ticker.match(/-T([\d.]+)$/);
  return match ? parseFloat(match[1]) : 0;
}

function fmtStrike(ticker: string): string {
  const strike = extractStrike(ticker);
  return strike ? "$" + Math.round(strike).toLocaleString() : ticker;
}

function fmtPrice(v: number | null): string {
  if (v == null) return "-";
  return `$${v.toFixed(2)}`;
}

function fmtInt(v: number | null): string {
  return v != null ? v.toLocaleString() : "-";
}

/** Format a UTC date as human-readable EST time, e.g. "Mar 4 5pm EST" */
function fmtEventTime(dateStr: string): string {
  const d = new Date(dateStr);
  return d.toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    timeZone: "America/New_York",
  }) + " EST";
}

/** Live countdown hook — updates every second */
function useCountdown(targetDate: string | null): string {
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (!targetDate) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [targetDate]);

  if (!targetDate) return "";
  const diff = new Date(targetDate).getTime() - now;
  if (diff <= 0) return "closed";

  const days = Math.floor(diff / 86400000);
  const hours = Math.floor((diff % 86400000) / 3600000);
  const mins = Math.floor((diff % 3600000) / 60000);
  const secs = Math.floor((diff % 60000) / 1000);

  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${mins}m`;
  return `${mins}m ${secs}s`;
}

function countdownColor(targetDate: string | null): string {
  if (!targetDate) return "text-fg-muted";
  const diff = new Date(targetDate).getTime() - Date.now();
  if (diff <= 0) return "text-fg-subtle";
  if (diff < 15 * 60 * 1000) return "text-red";
  if (diff < 60 * 60 * 1000) return "text-yellow";
  return "text-fg-muted";
}

// --- Main content ---

function CryptoContent() {
  const [slideOverMarket, setSlideOverMarket] = useState<MonitorMarket | null>(null);

  // 1. Fetch all crypto series
  const { data: allSeries } = useSeries("Crypto");

  // Sort: hourly series (active_events >= 3) first, then others
  const sortedSeries = useMemo(() => {
    if (!allSeries) return [];
    const hourly = allSeries.filter((s) => (s.active_events ?? 0) >= 3);
    const other = allSeries.filter((s) => (s.active_events ?? 0) < 3);
    return [...hourly, ...other];
  }, [allSeries]);

  // 2. Auto-select series: URL param > localStorage > first hourly
  const [selectedSeries, setSelectedSeries] = useState<string | null>(() => {
    const fromUrl = getUrlParam("series");
    if (fromUrl) return fromUrl;
    try {
      const stored = localStorage.getItem(SERIES_LS_KEY);
      if (stored) return stored;
    } catch {}
    return null;
  });

  useEffect(() => {
    if (selectedSeries) return; // already set
    if (sortedSeries.length > 0) {
      setSelectedSeries(sortedSeries[0].ticker);
    }
  }, [selectedSeries, sortedSeries]);

  const selectSeries = useCallback((ticker: string) => {
    setSelectedSeries(ticker);
    setSelectedEvent(null);
    try { localStorage.setItem(SERIES_LS_KEY, ticker); } catch {}
    setUrlParams({ series: ticker, event: null });
  }, []);

  // 3. Fetch events for selected series
  const { data: events } = useEvents(selectedSeries);

  // Sort events by close date (soonest first), only future events
  const futureEvents = useMemo(() => {
    if (!events) return [];
    const now = Date.now();
    return events
      .filter((e) => e.status === "active" && e.strike_date && new Date(e.strike_date).getTime() > now)
      .sort((a, b) => new Date(a.strike_date!).getTime() - new Date(b.strike_date!).getTime());
  }, [events]);

  // 4. Auto-select event: URL param > soonest closing
  const [selectedEvent, setSelectedEvent] = useState<string | null>(() => getUrlParam("event"));

  useEffect(() => {
    if (selectedEvent && events?.some((e) => e.ticker === selectedEvent)) return; // still valid
    if (futureEvents.length > 0) {
      setSelectedEvent(futureEvents[0].ticker);
    }
  }, [selectedEvent, events, futureEvents]);

  const selectEvent = useCallback((ticker: string) => {
    setSelectedEvent(ticker);
    setUrlParams({ event: ticker });
  }, []);

  // 5. Fetch markets for selected event
  const { data: markets } = useMarkets(selectedEvent);

  // Positions
  const { data: positions } = usePositions();

  const positionTickers = useMemo(() => {
    if (!positions) return new Set<string>();
    const set = new Set<string>();
    for (const p of positions.exchange) set.add(p.ticker);
    for (const p of positions.local) set.add(p.ticker);
    return set;
  }, [positions]);

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-bold">Crypto</h1>

      {/* Series pills */}
      {sortedSeries.length > 0 && (
        <SeriesPillBar
          series={sortedSeries}
          selected={selectedSeries}
          onSelect={selectSeries}
        />
      )}

      {/* Event cards */}
      {futureEvents.length > 0 && (
        <EventSelector
          events={futureEvents}
          selected={selectedEvent}
          onSelect={selectEvent}
        />
      )}
      {selectedSeries && futureEvents.length === 0 && events && (
        <div className="bg-bg-raised border border-border rounded-lg p-4 text-center text-fg-subtle text-sm">
          No upcoming events for {selectedSeries}
        </div>
      )}

      {/* Strike table */}
      {selectedEvent && markets && (
        <StrikeTable
          eventTicker={selectedEvent}
          markets={markets}
          positionTickers={positionTickers}
          onMarketClick={(m) => setSlideOverMarket(m)}
        />
      )}
      {selectedEvent && !markets && (
        <div className="bg-bg-raised border border-border rounded-lg p-8 text-center text-fg-subtle text-sm">
          Loading markets...
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

// --- Series pill bar ---

function SeriesPillBar({
  series,
  selected,
  onSelect,
}: {
  series: MonitorSeries[];
  selected: string | null;
  onSelect: (ticker: string) => void;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);

  return (
    <div className="relative">
      <div
        ref={scrollRef}
        className="flex gap-2 overflow-x-auto pb-1 scrollbar-none"
      >
        {series.map((s) => (
          <button
            key={s.ticker}
            onClick={() => onSelect(s.ticker)}
            className={`shrink-0 px-3 py-1.5 rounded-full text-xs font-medium transition-colors ${
              selected === s.ticker
                ? "bg-accent text-bg"
                : "bg-bg-raised border border-border text-fg-muted hover:text-fg hover:border-fg-subtle"
            }`}
          >
            {s.ticker}
            {s.active_events != null && (
              <span className="ml-1 opacity-60">({s.active_events})</span>
            )}
          </button>
        ))}
      </div>
    </div>
  );
}

// --- Event selector cards ---

function EventSelector({
  events,
  selected,
  onSelect,
}: {
  events: MonitorEvent[];
  selected: string | null;
  onSelect: (ticker: string) => void;
}) {
  return (
    <div className="flex gap-2 overflow-x-auto pb-1 scrollbar-none">
      {events.map((e) => (
        <EventCard
          key={e.ticker}
          event={e}
          isSelected={selected === e.ticker}
          onSelect={() => onSelect(e.ticker)}
        />
      ))}
    </div>
  );
}

function EventCard({
  event,
  isSelected,
  onSelect,
}: {
  event: MonitorEvent;
  isSelected: boolean;
  onSelect: () => void;
}) {
  const countdown = useCountdown(event.strike_date ?? null);
  const cdColor = countdownColor(event.strike_date ?? null);

  return (
    <button
      onClick={onSelect}
      className={`shrink-0 rounded-lg border px-4 py-3 text-left transition-colors min-w-[160px] ${
        isSelected
          ? "border-accent bg-accent/10"
          : "border-border bg-bg-raised hover:border-fg-subtle"
      }`}
    >
      <div className="text-sm font-medium text-fg">
        {event.strike_date ? fmtEventTime(event.strike_date) : event.ticker}
      </div>
      <div className={`text-xs font-mono mt-1 ${cdColor}`}>
        {countdown}
      </div>
      <div className="text-xs text-fg-subtle mt-1">
        {event.market_count ?? 0} markets
      </div>
    </button>
  );
}

// --- Strike table ---

function StrikeTable({
  eventTicker,
  markets,
  positionTickers,
  onMarketClick,
}: {
  eventTicker: string;
  markets: MonitorMarket[];
  positionTickers: Set<string>;
  onMarketClick: (m: MonitorMarket) => void;
}) {
  // Sort by strike descending
  const sorted = useMemo(() => {
    return [...markets].sort((a, b) => extractStrike(b.ticker) - extractStrike(a.ticker));
  }, [markets]);

  return (
    <div>
      <h2 className="text-sm font-medium text-fg-muted mb-2">
        Markets for {eventTicker}
        <span className="ml-2 text-fg-subtle">({sorted.length})</span>
      </h2>
      <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-fg-muted border-b border-border">
                <th className="px-4 py-2">Strike</th>
                <th className="px-4 py-2 text-right">Bid</th>
                <th className="px-4 py-2 text-right">Ask</th>
                <th className="px-4 py-2 text-right">Last</th>
                <th className="px-4 py-2 text-right">Vol</th>
                <th className="px-4 py-2 text-right">OI</th>
                <th className="px-4 py-2 w-6"></th>
              </tr>
            </thead>
            <tbody>
              {sorted.map((m) => {
                const bid = m.yes_bid ?? null;
                const ask = m.yes_ask ?? null;
                const mid = bid != null && ask != null ? (bid + ask) / 2 : null;
                const isAtm = mid != null && mid >= 0.30 && mid <= 0.70;
                const hasPosition = positionTickers.has(m.ticker);

                return (
                  <tr
                    key={m.ticker}
                    className={`border-b border-border-subtle hover:bg-bg-surface-hover cursor-pointer ${
                      isAtm ? "bg-accent/5" : ""
                    }`}
                    onClick={() => onMarketClick(m)}
                  >
                    <td className="px-4 py-1.5 font-mono text-xs">
                      {fmtStrike(m.ticker)}
                      {hasPosition && (
                        <span className="ml-1 text-accent text-xs" title="Has position">●</span>
                      )}
                      {isAtm && (
                        <span className="ml-1 text-xs text-fg-subtle">ATM</span>
                      )}
                    </td>
                    <td className="px-4 py-1.5 font-mono text-right text-xs">{fmtPrice(bid)}</td>
                    <td className="px-4 py-1.5 font-mono text-right text-xs">{fmtPrice(ask)}</td>
                    <td className="px-4 py-1.5 font-mono text-right text-xs">{fmtPrice(m.last ?? null)}</td>
                    <td className="px-4 py-1.5 font-mono text-right text-xs">{fmtInt(m.volume ?? null)}</td>
                    <td className="px-4 py-1.5 font-mono text-right text-xs">{fmtInt(m.open_interest ?? null)}</td>
                    <td className="px-4 py-1.5 text-fg-muted">&rarr;</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
