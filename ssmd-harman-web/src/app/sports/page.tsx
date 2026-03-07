"use client";

import { useState, useMemo, useCallback, useEffect, Fragment } from "react";
import useSWR from "swr";
import { useSeries, useEvents, useMarkets, usePositions } from "@/lib/hooks";
import { getEvents } from "@/lib/api";
import type { MonitorSeries, MonitorEvent, MonitorMarket, Side, Action } from "@/lib/types";
import { MarketSlideOver } from "@/components/market-slide-over";
import { usePinnedEvents } from "@/lib/pinned-events";

const SERIES_LS_KEY = "sports-selected-series";
const TODAY_KEY = "__TODAY__";

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

/** Extract series ticker from event ticker, e.g. "KXNBAGAME-26MAR05UTAWAS" → "KXNBAGAME" */
function eventSeries(eventTicker: string): string {
  const idx = eventTicker.indexOf("-");
  return idx > 0 ? eventTicker.substring(0, idx) : eventTicker;
}

function parseMatchup(title: string): string {
  return title.replace(/\s*[Ww]inner\??$/, "").replace(" at ", " @ ");
}

/** Parse game date from event ticker, e.g. "KXNBAGAME-26MAR05UTAWAS" → "2026-03-05" */
function parseGameDate(ticker: string): string | null {
  const match = ticker.match(/-(\d{2})(JAN|FEB|MAR|APR|MAY|JUN|JUL|AUG|SEP|OCT|NOV|DEC)(\d{2})/);
  if (!match) return null;
  const months: Record<string, string> = {
    JAN: "01", FEB: "02", MAR: "03", APR: "04", MAY: "05", JUN: "06",
    JUL: "07", AUG: "08", SEP: "09", OCT: "10", NOV: "11", DEC: "12",
  };
  return `20${match[1]}-${months[match[2]]}-${match[3]}`;
}

/** Returns precise datetime if available, otherwise date-only string (no time) */
function getEventDate(event: MonitorEvent): string | null {
  if (event.strike_date) return event.strike_date;
  return parseGameDate(event.ticker);
}

/** Returns true if the date string includes a time component (not just YYYY-MM-DD) */
function hasPreciseTime(dateStr: string): boolean {
  return dateStr.includes("T");
}

function isToday(dateStr: string): boolean {
  // For date-only strings like "2026-03-05", compare directly to avoid timezone issues
  if (!dateStr.includes("T")) {
    const todayStr = new Date().toLocaleDateString("en-CA"); // YYYY-MM-DD
    return dateStr === todayStr;
  }
  const d = new Date(dateStr);
  return d.toDateString() === new Date().toDateString();
}

function fmtGameDate(dateStr: string): string {
  const d = new Date(dateStr);
  const today = new Date();
  const tomorrow = new Date(today);
  tomorrow.setDate(tomorrow.getDate() + 1);

  if (d.toDateString() === today.toDateString()) return "Today";
  if (d.toDateString() === tomorrow.toDateString()) return "Tomorrow";
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

// --- Game state from expected_expiration_time ---

/** Estimated game duration by league (hours) — used to compute start time from EET */
const GAME_DURATION_HOURS: Record<string, number> = {
  NBA: 2.5, NHL: 2.5, NFL: 3.5, NCAAM: 2.5, NCAAW: 2.5, "NCAA BB": 2.5,
  "NCAA Hockey": 2.5, EPL: 2, "La Liga": 2, UCL: 2, "Serie A": 2,
  Bundesliga: 2, "Ligue 1": 2, MLS: 2, "MLB Spring": 3, WBC: 3,
  ATP: 2.5, WTA: 2, CS2: 2, Valorant: 2, LoL: 1.5, "Dota 2": 1.5,
  "NCAA Lax": 2, "Liga MX": 2, Europa: 2, AFL: 2.5, NRL: 2,
};

type GameState = "upcoming" | "in_progress" | "final" | "unknown";

/** Compute estimated start time from EET and league duration */
function getEstimatedStart(markets: MonitorMarket[] | null, eventTicker: string): Date | null {
  const eet = markets?.[0]?.expected_expiration_time;
  if (!eet) return null;
  const league = seriesLabel(eventSeries(eventTicker));
  const durationMs = (GAME_DURATION_HOURS[league] ?? 2.5) * 3600000;
  return new Date(new Date(eet).getTime() - durationMs);
}

function getGameState(
  markets: MonitorMarket[] | null,
  eventTicker: string,
): { state: GameState; countdownTarget: string | null; label: string } {
  if (!markets || markets.length === 0) {
    return { state: "unknown", countdownTarget: null, label: "" };
  }

  // Check lifecycle events first (authoritative signal from exchange)
  for (const m of markets) {
    if (m.lifecycle_events && m.lifecycle_events.length > 0) {
      const hasDetermined = m.lifecycle_events.some(
        (e) => e.type === "determined" || e.type === "settled"
      );
      if (hasDetermined) {
        return { state: "final", countdownTarget: null, label: "Final" };
      }

      // Check for active halt (deactivated without subsequent activated)
      const lastDeactivated = [...m.lifecycle_events].reverse().find((e) => e.type === "deactivated");
      const lastActivated = [...m.lifecycle_events].reverse().find((e) => e.type === "activated");
      if (lastDeactivated && (!lastActivated || new Date(lastDeactivated.ts) > new Date(lastActivated.ts))) {
        return { state: "unknown", countdownTarget: null, label: "Halted" };
      }
    }
  }

  // Fallback: price heuristic for Final (when lifecycle data hasn't arrived yet)
  const decided = markets.some(m => (m.yes_bid ?? 0) >= 0.95 || (m.yes_bid ?? 1) <= 0.05);
  if (decided) return { state: "final", countdownTarget: null, label: "Final" };

  // EET-based Live/Upcoming detection (unchanged)
  const estimatedStart = getEstimatedStart(markets, eventTicker);
  if (!estimatedStart) {
    return { state: "unknown", countdownTarget: null, label: "" };
  }

  const eetTime = new Date(markets[0].expected_expiration_time!).getTime();
  const now = Date.now();

  if (now < estimatedStart.getTime()) {
    return { state: "upcoming", countdownTarget: estimatedStart.toISOString(), label: "" };
  }
  if (now < eetTime + 3600000) {
    return { state: "in_progress", countdownTarget: null, label: "Live" };
  }
  return { state: "final", countdownTarget: null, label: "Final" };
}

/** Format estimated start time as local time, e.g. "7:00 PM" */
function fmtStartTime(markets: MonitorMarket[] | null, eventTicker: string): string {
  const start = getEstimatedStart(markets, eventTicker);
  if (!start) return "-";
  return start.toLocaleTimeString("en-US", {
    hour: "numeric",
    minute: "2-digit",
    timeZone: "America/New_York",
  });
}

// --- "Today" cross-series hook ---

/** Fetch events for multiple series and merge, filtered to today only */
function useTodayEvents(seriesTickers: string[] | null) {
  return useSWR(
    seriesTickers && seriesTickers.length > 0
      ? `sports-today-${seriesTickers.join(",")}`
      : null,
    async () => {
      const results = await Promise.all(
        seriesTickers!.map((s) => getEvents(s).catch(() => []))
      );
      const all = results.flat();
      // Filter to today's games only
      return all.filter((e) => {
        if (e.status !== "active") return false;
        const date = getEventDate(e);
        return date && isToday(date);
      });
    },
    { refreshInterval: 60000, keepPreviousData: true, dedupingInterval: 5000 }
  );
}

// --- Main ---

export default function SportsPage() {
  return <SportsContent />;
}

function SportsContent() {
  const [slideOver, setSlideOver] = useState<{
    market: MonitorMarket;
    side: Side;
    action: Action;
    price: string;
  } | null>(null);
  const [expandedEvent, setExpandedEvent] = useState<string | null>(null);

  // 1. Fetch all Sports series, sort by active events
  const { data: allSeries } = useSeries("Sports");
  const sortedSeries = useMemo(() => {
    if (!allSeries) return [];
    return [...allSeries]
      .filter((s) => (s.active_events ?? 0) > 0)
      .sort((a, b) => (b.active_events ?? 0) - (a.active_events ?? 0));
  }, [allSeries]);

  // 2. Auto-select series: URL > localStorage > "Today"
  const [selectedSeries, setSelectedSeries] = useState<string | null>(
    () => getUrlParam("series") || TODAY_KEY
  );

  // Category filter for Today mode (multi-select)
  const [categoryFilter, setCategoryFilter] = useState<Set<string>>(() => {
    const param = getUrlParam("cat");
    return param ? new Set(param.split(",")) : new Set();
  });

  useEffect(() => {
    if (selectedSeries) return;
    try {
      const stored = localStorage.getItem(SERIES_LS_KEY);
      if (stored && (stored === TODAY_KEY || allSeries?.some((s) => s.ticker === stored))) {
        setSelectedSeries(stored);
        return;
      }
    } catch {}
    setSelectedSeries(TODAY_KEY);
  }, [selectedSeries, allSeries]);

  const selectSeries = useCallback((ticker: string) => {
    setSelectedSeries(ticker);
    setExpandedEvent(null);
    if (ticker !== TODAY_KEY) setCategoryFilter(new Set());
    try {
      localStorage.setItem(SERIES_LS_KEY, ticker);
    } catch {}
    setUrlParams({ series: ticker, cat: null });
  }, []);

  const toggleCategory = useCallback((seriesTicker: string) => {
    setCategoryFilter((prev) => {
      const next = new Set(prev);
      if (next.has(seriesTicker)) next.delete(seriesTicker);
      else next.add(seriesTicker);
      setUrlParams({ cat: next.size > 0 ? [...next].join(",") : null });
      return next;
    });
  }, []);

  const isTodayMode = selectedSeries === TODAY_KEY;

  // 3a. "Today" mode: fetch events across top series
  const todaySeriesTickers = useMemo(() => {
    if (!isTodayMode || !sortedSeries.length) return null;
    // Fetch top 20 series by active events to keep API calls reasonable
    return sortedSeries.slice(0, 20).map((s) => s.ticker);
  }, [isTodayMode, sortedSeries]);

  const { data: todayEvents } = useTodayEvents(todaySeriesTickers);

  // 3b. Single-series mode: fetch events normally
  const { data: singleSeriesEvents } = useEvents(
    isTodayMode ? null : selectedSeries
  );

  // Leagues present in today's events (for category filter chips)
  const todayLeagues = useMemo(() => {
    if (!todayEvents) return [];
    const counts = new Map<string, number>();
    for (const e of todayEvents) {
      const s = eventSeries(e.ticker);
      counts.set(s, (counts.get(s) ?? 0) + 1);
    }
    return [...counts.entries()]
      .sort((a, b) => b[1] - a[1])
      .map(([ticker, count]) => ({ ticker, count }));
  }, [todayEvents]);

  // Compute displayed events
  const displayEvents = useMemo(() => {
    if (isTodayMode) {
      if (!todayEvents) return [];
      let filtered = todayEvents;
      if (categoryFilter.size > 0) {
        filtered = filtered.filter((e) => categoryFilter.has(eventSeries(e.ticker)));
      }
      return [...filtered].sort((a, b) => {
        const da = getEventDate(a);
        const db = getEventDate(b);
        if (!da && !db) return 0;
        if (!da) return 1;
        if (!db) return -1;
        return new Date(da).getTime() - new Date(db).getTime();
      });
    }

    if (!singleSeriesEvents) return [];
    const todayStr = new Date().toLocaleDateString("en-CA"); // YYYY-MM-DD
    const now = Date.now();
    return singleSeriesEvents
      .filter((e) => {
        if (e.status !== "active") return false;
        const date = getEventDate(e);
        if (!date) return true;
        // Date-only: show if today or future
        if (!date.includes("T")) return date >= todayStr;
        return new Date(date).getTime() > now;
      })
      .sort((a, b) => {
        const da = getEventDate(a);
        const db = getEventDate(b);
        if (!da && !db) return 0;
        if (!da) return 1;
        if (!db) return -1;
        return new Date(da).getTime() - new Date(db).getTime();
      });
  }, [isTodayMode, todayEvents, singleSeriesEvents, categoryFilter]);

  // Auto-expand first game
  useEffect(() => {
    if (displayEvents.length > 0 && !expandedEvent) {
      setExpandedEvent(displayEvents[0].ticker);
    }
  }, [displayEvents, expandedEvent]);

  // Pinned events
  const { isPinned, toggle: togglePin } = usePinnedEvents();

  // Positions
  const { data: positions } = usePositions();

  const positionTickers = useMemo(() => {
    if (!positions) return new Set<string>();
    const set = new Set<string>();
    for (const p of positions.exchange) set.add(p.ticker);
    for (const p of positions.local) set.add(p.ticker);
    return set;
  }, [positions]);

  const toggleExpand = (eventTicker: string) => {
    setExpandedEvent((prev) => (prev === eventTicker ? null : eventTicker));
  };

  const eventsLoaded = isTodayMode ? todayEvents !== undefined : singleSeriesEvents !== undefined;

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-bold">
        Sports
        {displayEvents.length > 0 && (
          <span className="ml-2 text-sm font-normal text-fg-muted">
            ({displayEvents.length} games)
          </span>
        )}
      </h1>

      {/* Series pills — wrapping grid */}
      {sortedSeries.length > 0 && (
        <SeriesPillBar
          series={sortedSeries}
          selected={selectedSeries}
          onSelect={selectSeries}
          todayCount={todayEvents?.length ?? null}
        />
      )}

      {/* Category filter chips within Today mode */}
      {isTodayMode && todayLeagues.length > 1 && (
        <CategoryFilterBar
          leagues={todayLeagues}
          active={categoryFilter}
          onToggle={toggleCategory}
        />
      )}

      {/* Game list */}
      {displayEvents.length > 0 && (
        <GameList
          events={displayEvents}
          showLeague={isTodayMode}
          expandedEvent={expandedEvent}
          positionTickers={positionTickers}
          isPinned={isPinned}
          togglePin={togglePin}
          onToggleExpand={toggleExpand}
          onQuickTrade={(m, side, action, price) => setSlideOver({ market: m, side, action, price })}
        />
      )}
      {selectedSeries && displayEvents.length === 0 && eventsLoaded && (
        <div className="bg-bg-raised border border-border rounded-lg p-4 text-center text-fg-subtle text-sm">
          {isTodayMode
            ? categoryFilter.size > 0
              ? "No games today for selected categories"
              : "No games today"
            : `No upcoming games for ${seriesLabel(selectedSeries)}`}
        </div>
      )}
      {!eventsLoaded && selectedSeries && (
        <div className="bg-bg-raised border border-border rounded-lg p-8 text-center text-fg-subtle text-sm">
          Loading games...
        </div>
      )}

      {/* Slide-over */}
      {slideOver && (
        <MarketSlideOver
          market={slideOver.market}
          onClose={() => setSlideOver(null)}
          initialSide={slideOver.side}
          initialAction={slideOver.action}
          initialPrice={slideOver.price}
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
  todayCount,
}: {
  series: MonitorSeries[];
  selected: string | null;
  onSelect: (ticker: string) => void;
  todayCount: number | null;
}) {
  return (
    <div className="flex flex-wrap gap-2">
      {/* "Today" pill */}
      <button
        onClick={() => onSelect(TODAY_KEY)}
        className={`px-3 py-1.5 rounded-full text-xs font-medium transition-colors ${
          selected === TODAY_KEY
            ? "bg-accent text-bg"
            : "bg-bg-raised border border-border text-fg-muted hover:text-fg hover:border-fg-subtle"
        }`}
      >
        Today
        {todayCount != null && (
          <span className="ml-1 opacity-60">({todayCount})</span>
        )}
      </button>

      {series.map((s) => (
        <button
          key={s.ticker}
          onClick={() => onSelect(s.ticker)}
          className={`px-3 py-1.5 rounded-full text-xs font-medium transition-colors ${
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
  );
}

function CategoryFilterBar({
  leagues,
  active,
  onToggle,
}: {
  leagues: { ticker: string; count: number }[];
  active: Set<string>;
  onToggle: (ticker: string) => void;
}) {
  return (
    <div className="flex flex-wrap gap-1.5">
      <span className="text-xs text-fg-subtle py-1 mr-1">Filter:</span>
      {leagues.map(({ ticker, count }) => (
        <button
          key={ticker}
          onClick={() => onToggle(ticker)}
          className={`px-2 py-1 rounded text-xs transition-colors ${
            active.has(ticker)
              ? "bg-accent/20 text-accent border border-accent/40"
              : "bg-bg-surface border border-border-subtle text-fg-muted hover:text-fg hover:border-fg-subtle"
          }`}
        >
          {seriesLabel(ticker)}
          <span className="ml-1 opacity-60">{count}</span>
        </button>
      ))}
    </div>
  );
}

// --- Sortable columns ---

type SortColumn = "league" | "game" | "time" | "markets";
type SortDir = "asc" | "desc";

function getSortValue(event: MonitorEvent, col: SortColumn): string | number {
  switch (col) {
    case "league":
      return seriesLabel(eventSeries(event.ticker));
    case "game":
      return parseMatchup(event.title ?? event.ticker);
    case "time": {
      const d = getEventDate(event);
      return d ? new Date(d).getTime() : Infinity;
    }
    case "markets":
      return event.market_count ?? 0;
  }
}

function SortHeader({
  label,
  column,
  current,
  dir,
  onSort,
  className,
}: {
  label: string;
  column: SortColumn;
  current: SortColumn;
  dir: SortDir;
  onSort: (col: SortColumn) => void;
  className?: string;
}) {
  const active = current === column;
  return (
    <th
      className={`px-4 py-2 cursor-pointer select-none hover:text-fg ${className ?? ""}`}
      onClick={() => onSort(column)}
    >
      {label}
      {active && <span className="ml-1">{dir === "asc" ? "\u25B4" : "\u25BE"}</span>}
    </th>
  );
}

// --- Game list ---

// Removed useAllEventMarkets — was fetching markets for ALL events in parallel,
// crushing the proxy. GameRow fetches markets only when expanded.

function GameList({
  events,
  showLeague,
  expandedEvent,
  positionTickers,
  isPinned,
  togglePin,
  onToggleExpand,
  onQuickTrade,
}: {
  events: MonitorEvent[];
  showLeague: boolean;
  expandedEvent: string | null;
  positionTickers: Set<string>;
  isPinned: (ticker: string) => boolean;
  togglePin: (ticker: string) => void;
  onToggleExpand: (eventTicker: string) => void;
  onQuickTrade: (m: MonitorMarket, side: Side, action: Action, price: string) => void;
}) {
  const [sortCol, setSortCol] = useState<SortColumn>("time");
  const [sortDir, setSortDir] = useState<SortDir>("asc");

  const handleSort = useCallback((col: SortColumn) => {
    setSortCol((prev) => {
      if (prev === col) {
        setSortDir((d) => (d === "asc" ? "desc" : "asc"));
        return col;
      }
      setSortDir(col === "markets" ? "desc" : "asc");
      return col;
    });
  }, []);

  const sortedEvents = useMemo(() => {
    return [...events].sort((a, b) => {
      // Pinned events always sort to top
      const aPinned = isPinned(a.ticker) ? 0 : 1;
      const bPinned = isPinned(b.ticker) ? 0 : 1;
      if (aPinned !== bPinned) return aPinned - bPinned;

      // Within same group, apply normal sort
      const va = getSortValue(a, sortCol);
      const vb = getSortValue(b, sortCol);
      const cmp =
        typeof va === "number" && typeof vb === "number"
          ? va - vb
          : String(va).localeCompare(String(vb));
      return sortDir === "asc" ? cmp : -cmp;
    });
  }, [events, sortCol, sortDir, isPinned]);

  return (
    <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-xs text-fg-muted border-b border-border">
              <th className="px-4 py-2 w-6"></th>
              {showLeague && (
                <SortHeader label="League" column="league" current={sortCol} dir={sortDir} onSort={handleSort} />
              )}
              <SortHeader label="Game" column="game" current={sortCol} dir={sortDir} onSort={handleSort} />
              <SortHeader label="Start" column="time" current={sortCol} dir={sortDir} onSort={handleSort} className="text-right" />
              <th className="px-4 py-2 text-right">Status</th>
              <SortHeader label="Markets" column="markets" current={sortCol} dir={sortDir} onSort={handleSort} className="text-right" />
              <th className="px-4 py-2 w-6"></th>
            </tr>
          </thead>
          <tbody>
            {sortedEvents.map((e, i) => {
              const showSeparator =
                i > 0 && isPinned(sortedEvents[i - 1].ticker) && !isPinned(e.ticker);
              return (
                <Fragment key={e.ticker}>
                  {showSeparator && (
                    <tr>
                      <td colSpan={showLeague ? 7 : 6} className="h-px bg-accent/20" />
                    </tr>
                  )}
                  <GameRow
                    event={e}
                    showLeague={showLeague}
                    isExpanded={expandedEvent === e.ticker}
                    positionTickers={positionTickers}
                    isPinned={isPinned(e.ticker)}
                    onTogglePin={() => togglePin(e.ticker)}
                    onToggle={() => onToggleExpand(e.ticker)}
                    onQuickTrade={onQuickTrade}
                  />
                </Fragment>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

// --- Game row + expandable markets ---

function GameRow({
  event,
  showLeague,
  isExpanded,
  positionTickers,
  isPinned,
  onTogglePin,
  onToggle,
  onQuickTrade,
}: {
  event: MonitorEvent;
  showLeague: boolean;
  isExpanded: boolean;
  positionTickers: Set<string>;
  isPinned: boolean;
  onTogglePin: () => void;
  onToggle: () => void;
  onQuickTrade: (m: MonitorMarket, side: Side, action: Action, price: string) => void;
}) {
  // Only fetch markets when expanded — prevents 429 rate limit flood
  const { data: markets } = useMarkets(isExpanded ? event.ticker : null);
  const eventDate = getEventDate(event);

  // Game state: use market data when available, otherwise show date info
  const gameState = useMemo(
    () => markets ? getGameState(markets, event.ticker) : { state: "unknown" as GameState, countdownTarget: null, label: "" },
    [markets, event.ticker],
  );

  const countdown = useCountdown(gameState.countdownTarget);
  const colSpan = showLeague ? 7 : 6;

  // Color based on game state
  const stateColor =
    gameState.state === "in_progress" ? "text-green" :
    gameState.state === "final" ? "text-fg-subtle" :
    gameState.state === "upcoming" && gameState.countdownTarget
      ? countdownColor(gameState.countdownTarget)
      : "text-fg-muted";

  return (
    <>
      <tr
        className={`group border-b border-border-subtle hover:bg-bg-surface-hover cursor-pointer ${
          isExpanded ? "bg-accent/5" : ""
        }`}
        onClick={onToggle}
      >
        <td className="px-2 py-2">
          <span className="inline-flex items-center gap-1">
            <button
              onClick={(e) => { e.stopPropagation(); onTogglePin(); }}
              className={`text-xs leading-none ${isPinned ? "text-accent" : "text-transparent group-hover:text-fg-subtle hover:!text-accent"} transition-colors`}
              title={isPinned ? "Unpin" : "Pin to top"}
            >
              &#9650;
            </button>
            <span className={`text-xs transition-transform inline-block ${isExpanded ? "rotate-90" : ""}`}>
              &#9654;
            </span>
          </span>
        </td>
        {showLeague && (
          <td className="px-4 py-2">
            <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded bg-bg-surface text-fg-muted">
              {seriesLabel(eventSeries(event.ticker))}
            </span>
          </td>
        )}
        <td className="px-4 py-2 font-medium">
          {parseMatchup(event.title ?? event.ticker)}
        </td>
        <td className="px-4 py-2 text-right text-xs text-fg-muted">
          {markets ? fmtStartTime(markets, event.ticker) : (eventDate ? fmtGameDate(eventDate) : "-")}
        </td>
        <td className={`px-4 py-2 text-right text-xs font-mono ${stateColor}`}>
          {countdown || gameState.label}
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
            const yesBid = m.yes_bid ?? null;
            const yesAsk = m.yes_ask ?? null;
            const noBid = yesAsk != null ? +(1 - yesAsk).toFixed(2) : null;
            const noAsk = yesBid != null ? +(1 - yesBid).toFixed(2) : null;
            const yesLast = m.last ?? null;
            const noLast = yesLast != null ? +(1 - yesLast).toFixed(2) : null;
            const hasPosition = positionTickers.has(m.ticker);
            const teamName = m.ticker.split("-").pop() ?? m.ticker;

            return (
              <Fragment key={m.ticker}>
                {/* Yes row */}
                <tr className="border-b border-border-subtle bg-bg-surface hover:bg-bg-surface-hover">
                  <td className="px-4 py-1.5"></td>
                  <td className="px-4 py-1.5 pl-10" colSpan={showLeague ? 2 : 1}>
                    <span className="font-mono text-xs">{teamName}</span>
                    {hasPosition && (
                      <span className="ml-1 text-accent text-xs" title="Has position">●</span>
                    )}
                    <span className="ml-2 text-[10px] font-medium text-green">YES</span>
                  </td>
                  <td className="px-4 py-1.5 text-right font-mono text-xs">
                    {fmtPrice(yesBid)} <span className="text-fg-subtle">/</span> {fmtPrice(yesAsk)}
                  </td>
                  <td className="px-4 py-1.5 text-right font-mono text-xs">
                    {fmtPrice(yesLast)}
                  </td>
                  <td className="px-4 py-1.5 text-right font-mono text-xs">
                    {fmtInt(m.volume ?? null)}
                  </td>
                  <td className="px-2 py-1.5">
                    <div className="flex gap-1">
                      <button
                        onClick={() => onQuickTrade(m, "yes", "buy", yesAsk != null ? yesAsk.toFixed(2) : "")}
                        className="px-2 py-0.5 rounded text-[10px] font-medium bg-green/10 text-green hover:bg-green/20 transition-colors"
                        title="Buy Yes @ ask"
                      >
                        Buy
                      </button>
                      <button
                        onClick={() => onQuickTrade(m, "yes", "sell", yesBid != null ? yesBid.toFixed(2) : "")}
                        className="px-2 py-0.5 rounded text-[10px] font-medium bg-red/10 text-red hover:bg-red/20 transition-colors"
                        title="Sell Yes @ bid"
                      >
                        Sell
                      </button>
                    </div>
                  </td>
                </tr>
                {/* No row */}
                <tr className="border-b border-border-subtle bg-bg-surface hover:bg-bg-surface-hover">
                  <td className="px-4 py-1.5"></td>
                  <td className="px-4 py-1.5 pl-10" colSpan={showLeague ? 2 : 1}>
                    <span className="font-mono text-xs">{teamName}</span>
                    <span className="ml-2 text-[10px] font-medium text-red">NO</span>
                  </td>
                  <td className="px-4 py-1.5 text-right font-mono text-xs">
                    {fmtPrice(noBid)} <span className="text-fg-subtle">/</span> {fmtPrice(noAsk)}
                  </td>
                  <td className="px-4 py-1.5 text-right font-mono text-xs">
                    {fmtPrice(noLast)}
                  </td>
                  <td className="px-4 py-1.5 text-right font-mono text-xs"></td>
                  <td className="px-2 py-1.5">
                    <div className="flex gap-1">
                      <button
                        onClick={() => onQuickTrade(m, "no", "buy", noAsk != null ? noAsk.toFixed(2) : "")}
                        className="px-2 py-0.5 rounded text-[10px] font-medium bg-green/10 text-green hover:bg-green/20 transition-colors"
                        title="Buy No @ ask"
                      >
                        Buy
                      </button>
                      <button
                        onClick={() => onQuickTrade(m, "no", "sell", noBid != null ? noBid.toFixed(2) : "")}
                        className="px-2 py-0.5 rounded text-[10px] font-medium bg-red/10 text-red hover:bg-red/20 transition-colors"
                        title="Sell No @ bid"
                      >
                        Sell
                      </button>
                    </div>
                  </td>
                </tr>
              </Fragment>
            );
          })}
        </>
      )}
      {isExpanded && !markets && (
        <tr className="border-b border-border-subtle bg-bg-surface">
          <td colSpan={colSpan} className="px-4 py-2 text-center text-fg-subtle text-xs">
            Loading markets...
          </td>
        </tr>
      )}
    </>
  );
}
