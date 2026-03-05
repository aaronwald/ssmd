"use client";

import { useState, useMemo, useCallback, useEffect } from "react";
import { useSeries, useEvents, useMarkets, usePositions } from "@/lib/hooks";
import { useWatchlist } from "@/lib/watchlist-context";
import type { MonitorSeries, MonitorEvent, MonitorMarket } from "@/lib/types";
import { MarketSlideOver } from "@/components/market-slide-over";

const SERIES_LS_KEY = "sports-selected-series";

function getUrlParam(key: string): string | null {
  if (typeof window === "undefined") return null;
  return new URLSearchParams(window.location.search).get(key);
}

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

// --- Helpers ---

const SERIES_LABELS: Record<string, string> = {
  KXNBAGAME: "NBA",
  KXNHLGAME: "NHL",
  KXNFLGAME: "NFL",
  KXNCAAMBGAME: "NCAAM",
  KXNCAAWBGAME: "NCAAW",
  KXNCAABGAME: "NCAA BB",
  KXNCAAHOCKEYGAME: "NCAA Hockey",
  KXNCAAMLAXGAME: "NCAA Lax",
  KXEPLGAME: "EPL",
  KXLALIGAGAME: "La Liga",
  KXUCLGAME: "UCL",
  KXUELGAME: "Europa",
  KXSERIEAGAME: "Serie A",
  KXBUNDESLIGAGAME: "Bundesliga",
  KXLIGUE1GAME: "Ligue 1",
  KXMLSGAME: "MLS",
  KXLIGAMXGAME: "Liga MX",
  KXWCGAME: "World Cup",
  KXAFLGAME: "AFL",
  KXNRLMATCH: "NRL",
  KXMLBSTGAME: "MLB Spring",
  KXWBCGAME: "WBC",
  KXATPMATCH: "ATP",
  KXWTAMATCH: "WTA",
  KXCS2GAME: "CS2",
  KXVALORANTGAME: "Valorant",
  KXLOLGAME: "LoL",
  KXDOTA2GAME: "Dota 2",
};

function seriesLabel(ticker: string): string {
  if (SERIES_LABELS[ticker]) return SERIES_LABELS[ticker];
  return ticker
    .replace(/^KX/, "")
    .replace(/GAME$/, "")
    .replace(/MATCH$/, "");
}

function parseMatchup(title: string): string {
  return title.replace(/\s*[Ww]inner\??$/, "").replace(" at ", " @ ");
}

/** Parse game date from event ticker, e.g. "KXNBAGAME-26MAR05UTAWAS" → "2026-03-05" */
function parseGameDate(ticker: string): string | null {
  // Match pattern: -YYMMMDD (e.g., -26MAR05, -26MAR07)
  const match = ticker.match(/-(\d{2})(JAN|FEB|MAR|APR|MAY|JUN|JUL|AUG|SEP|OCT|NOV|DEC)(\d{2})/);
  if (!match) return null;
  const months: Record<string, string> = {
    JAN: "01", FEB: "02", MAR: "03", APR: "04", MAY: "05", JUN: "06",
    JUL: "07", AUG: "08", SEP: "09", OCT: "10", NOV: "11", DEC: "12",
  };
  const year = `20${match[1]}`;
  const month = months[match[2]];
  const day = match[3];
  return `${year}-${month}-${day}`;
}

/** Get displayable date for a sports event (from strike_date or parsed ticker) */
function getEventDate(event: MonitorEvent): string | null {
  if (event.strike_date) return event.strike_date;
  const parsed = parseGameDate(event.ticker);
  if (parsed) return `${parsed}T23:59:00Z`; // EOD as approximate
  return null;
}

function fmtGameDate(dateStr: string): string {
  const d = new Date(dateStr);
  const today = new Date();
  const tomorrow = new Date(today);
  tomorrow.setDate(tomorrow.getDate() + 1);

  const isToday = d.toDateString() === today.toDateString();
  const isTomorrow = d.toDateString() === tomorrow.toDateString();

  if (isToday) return "Today";
  if (isTomorrow) return "Tomorrow";
  return d.toLocaleDateString("en-US", {
    weekday: "short",
    month: "short",
    day: "numeric",
    timeZone: "America/New_York",
  });
}

function fmtPrice(v: number | null): string {
  if (v == null) return "-";
  return `$${v.toFixed(2)}`;
}

function fmtInt(v: number | null): string {
  return v != null ? v.toLocaleString() : "-";
}

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

// --- Main ---

export default function SportsPage() {
  return <SportsContent />;
}

function SportsContent() {
  const [slideOverMarket, setSlideOverMarket] = useState<MonitorMarket | null>(
    null
  );
  const [expandedEvent, setExpandedEvent] = useState<string | null>(null);

  // 1. Fetch all Sports series, sort by active events
  const { data: allSeries } = useSeries("Sports");
  const sortedSeries = useMemo(() => {
    if (!allSeries) return [];
    return [...allSeries].sort(
      (a, b) => (b.active_events ?? 0) - (a.active_events ?? 0)
    );
  }, [allSeries]);

  // 2. Auto-select series: URL > localStorage > first
  const [selectedSeries, setSelectedSeries] = useState<string | null>(() =>
    getUrlParam("series")
  );

  useEffect(() => {
    if (selectedSeries) return;
    try {
      const stored = localStorage.getItem(SERIES_LS_KEY);
      if (stored && allSeries?.some((s) => s.ticker === stored)) {
        setSelectedSeries(stored);
        return;
      }
    } catch {}
    if (sortedSeries.length > 0) {
      setSelectedSeries(sortedSeries[0].ticker);
    }
  }, [selectedSeries, allSeries, sortedSeries]);

  const selectSeries = useCallback((ticker: string) => {
    setSelectedSeries(ticker);
    setExpandedEvent(null);
    try {
      localStorage.setItem(SERIES_LS_KEY, ticker);
    } catch {}
    setUrlParams({ series: ticker });
  }, []);

  // 3. Fetch events for selected series
  const { data: events } = useEvents(selectedSeries);

  const futureEvents = useMemo(() => {
    if (!events) return [];
    const now = Date.now();
    return events
      .filter((e) => {
        if (e.status !== "active") return false;
        const date = getEventDate(e);
        // Include if we can't parse date (show all active) or if date is in the future
        return !date || new Date(date).getTime() > now;
      })
      .sort((a, b) => {
        const da = getEventDate(a);
        const db = getEventDate(b);
        if (!da && !db) return 0;
        if (!da) return 1;
        if (!db) return -1;
        return new Date(da).getTime() - new Date(db).getTime();
      });
  }, [events]);

  // Auto-expand first game
  useEffect(() => {
    if (futureEvents.length > 0 && !expandedEvent) {
      setExpandedEvent(futureEvents[0].ticker);
    }
  }, [futureEvents, expandedEvent]);

  // 4. Fetch markets for expanded game
  const { data: expandedMarkets } = useMarkets(expandedEvent);

  // Positions & watchlist
  const { data: positions } = usePositions();
  const watchlist = useWatchlist();

  const positionTickers = useMemo(() => {
    if (!positions) return new Set<string>();
    const set = new Set<string>();
    for (const p of positions.exchange) set.add(p.ticker);
    for (const p of positions.local) set.add(p.ticker);
    return set;
  }, [positions]);

  const toggleStar = (ticker: string, title?: string) => {
    if (watchlist.has(ticker)) {
      watchlist.remove(ticker);
    } else {
      watchlist.add({ ticker, exchange: "kalshi", title });
    }
  };

  const toggleExpand = (eventTicker: string) => {
    setExpandedEvent((prev) => (prev === eventTicker ? null : eventTicker));
  };

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-bold">Sports</h1>

      {/* Series pills */}
      {sortedSeries.length > 0 && (
        <SeriesPillBar
          series={sortedSeries}
          selected={selectedSeries}
          onSelect={selectSeries}
        />
      )}

      {/* Game list */}
      {futureEvents.length > 0 && (
        <GameList
          events={futureEvents}
          expandedEvent={expandedEvent}
          expandedMarkets={expandedMarkets ?? null}
          positionTickers={positionTickers}
          watchlist={watchlist}
          toggleStar={toggleStar}
          onToggleExpand={toggleExpand}
          onMarketClick={(m) => setSlideOverMarket(m)}
        />
      )}
      {selectedSeries && futureEvents.length === 0 && events && (
        <div className="bg-bg-raised border border-border rounded-lg p-4 text-center text-fg-subtle text-sm">
          No upcoming games for {seriesLabel(selectedSeries)}
        </div>
      )}
      {!events && selectedSeries && (
        <div className="bg-bg-raised border border-border rounded-lg p-8 text-center text-fg-subtle text-sm">
          Loading games...
        </div>
      )}

      {/* Slide-over */}
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
  return (
    <div className="relative">
      <div className="flex gap-2 overflow-x-auto pb-1 scrollbar-none">
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
            {seriesLabel(s.ticker)}
            {s.active_events != null && (
              <span className="ml-1 opacity-60">({s.active_events})</span>
            )}
          </button>
        ))}
      </div>
    </div>
  );
}

// --- Game list ---

function GameList({
  events,
  expandedEvent,
  expandedMarkets,
  positionTickers,
  watchlist,
  toggleStar,
  onToggleExpand,
  onMarketClick,
}: {
  events: MonitorEvent[];
  expandedEvent: string | null;
  expandedMarkets: MonitorMarket[] | null;
  positionTickers: Set<string>;
  watchlist: { has: (ticker: string) => boolean };
  toggleStar: (ticker: string, title?: string) => void;
  onToggleExpand: (eventTicker: string) => void;
  onMarketClick: (m: MonitorMarket) => void;
}) {
  return (
    <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-xs text-fg-muted border-b border-border">
              <th className="px-4 py-2 w-6"></th>
              <th className="px-4 py-2">Game</th>
              <th className="px-4 py-2 text-right">Time</th>
              <th className="px-4 py-2 text-right">Countdown</th>
              <th className="px-4 py-2 text-right">Markets</th>
              <th className="px-4 py-2 w-6"></th>
            </tr>
          </thead>
          <tbody>
            {events.map((e) => (
              <GameRow
                key={e.ticker}
                event={e}
                isExpanded={expandedEvent === e.ticker}
                markets={expandedEvent === e.ticker ? expandedMarkets : null}
                positionTickers={positionTickers}
                watchlist={watchlist}
                toggleStar={toggleStar}
                onToggle={() => onToggleExpand(e.ticker)}
                onMarketClick={onMarketClick}
              />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

// --- Game row + expandable markets ---

function GameRow({
  event,
  isExpanded,
  markets,
  positionTickers,
  watchlist,
  toggleStar,
  onToggle,
  onMarketClick,
}: {
  event: MonitorEvent;
  isExpanded: boolean;
  markets: MonitorMarket[] | null;
  positionTickers: Set<string>;
  watchlist: { has: (ticker: string) => boolean };
  toggleStar: (ticker: string, title?: string) => void;
  onToggle: () => void;
  onMarketClick: (m: MonitorMarket) => void;
}) {
  const eventDate = getEventDate(event);
  const countdown = useCountdown(eventDate);
  const cdColor = countdownColor(eventDate);

  return (
    <>
      <tr
        className={`border-b border-border-subtle hover:bg-bg-surface-hover cursor-pointer ${
          isExpanded ? "bg-accent/5" : ""
        }`}
        onClick={onToggle}
      >
        <td className="px-4 py-2">
          <span
            className={`text-xs transition-transform inline-block ${isExpanded ? "rotate-90" : ""}`}
          >
            &#9654;
          </span>
        </td>
        <td className="px-4 py-2 font-medium">
          {parseMatchup(event.title ?? event.ticker)}
        </td>
        <td className="px-4 py-2 text-right text-xs text-fg-muted">
          {eventDate ? fmtGameDate(eventDate) : "-"}
        </td>
        <td className={`px-4 py-2 text-right text-xs font-mono ${cdColor}`}>
          {countdown || "-"}
        </td>
        <td className="px-4 py-2 text-right text-xs text-fg-subtle">
          {event.market_count ?? 0}
        </td>
        <td className="px-4 py-2 text-fg-muted">&rarr;</td>
      </tr>

      {/* Expanded: show both team markets */}
      {isExpanded && markets && (
        <>
          {markets.map((m) => {
            const bid = m.yes_bid ?? null;
            const ask = m.yes_ask ?? null;
            const hasPosition = positionTickers.has(m.ticker);
            const teamName = m.ticker.split("-").pop() ?? m.ticker;

            return (
              <tr
                key={m.ticker}
                className="border-b border-border-subtle bg-bg-surface hover:bg-bg-surface-hover cursor-pointer"
                onClick={() => onMarketClick(m)}
              >
                <td className="px-4 py-1.5"></td>
                <td className="px-4 py-1.5 pl-10">
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      toggleStar(m.ticker, m.title);
                    }}
                    className={`text-sm mr-2 ${watchlist.has(m.ticker) ? "text-yellow" : "text-fg-subtle hover:text-yellow"}`}
                  >
                    {watchlist.has(m.ticker) ? "\u2605" : "\u2606"}
                  </button>
                  <span className="font-mono text-xs">{teamName}</span>
                  {hasPosition && (
                    <span
                      className="ml-1 text-accent text-xs"
                      title="Has position"
                    >
                      ●
                    </span>
                  )}
                </td>
                <td className="px-4 py-1.5 text-right font-mono text-xs">
                  {fmtPrice(bid)}{" "}
                  <span className="text-fg-subtle">/</span>{" "}
                  {fmtPrice(ask)}
                </td>
                <td className="px-4 py-1.5 text-right font-mono text-xs">
                  {fmtPrice(m.last ?? null)}
                </td>
                <td className="px-4 py-1.5 text-right font-mono text-xs">
                  {fmtInt(m.volume ?? null)}
                </td>
                <td className="px-4 py-1.5 text-fg-muted">&rarr;</td>
              </tr>
            );
          })}
        </>
      )}
      {isExpanded && !markets && (
        <tr className="border-b border-border-subtle bg-bg-surface">
          <td colSpan={6} className="px-4 py-2 text-center text-fg-subtle text-xs">
            Loading markets...
          </td>
        </tr>
      )}
    </>
  );
}
