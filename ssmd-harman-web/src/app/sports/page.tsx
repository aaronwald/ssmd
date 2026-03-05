"use client";

import { useState, useMemo, useCallback, useEffect } from "react";
import useSWR from "swr";
import { useSeries, useEvents, useMarkets, usePositions } from "@/lib/hooks";
import { useWatchlist } from "@/lib/watchlist-context";
import { getEvents } from "@/lib/api";
import type { MonitorSeries, MonitorEvent, MonitorMarket } from "@/lib/types";
import { MarketSlideOver } from "@/components/market-slide-over";

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
    { refreshInterval: 60000 }
  );
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
    return [...allSeries]
      .filter((s) => (s.active_events ?? 0) > 0)
      .sort((a, b) => (b.active_events ?? 0) - (a.active_events ?? 0));
  }, [allSeries]);

  // 2. Auto-select series: URL > localStorage > "Today"
  const [selectedSeries, setSelectedSeries] = useState<string | null>(
    () => getUrlParam("series") || TODAY_KEY
  );

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
    try {
      localStorage.setItem(SERIES_LS_KEY, ticker);
    } catch {}
    setUrlParams({ series: ticker });
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

  // Compute displayed events
  const displayEvents = useMemo(() => {
    if (isTodayMode) {
      if (!todayEvents) return [];
      return [...todayEvents].sort((a, b) => {
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
  }, [isTodayMode, todayEvents, singleSeriesEvents]);

  // Auto-expand first game
  useEffect(() => {
    if (displayEvents.length > 0 && !expandedEvent) {
      setExpandedEvent(displayEvents[0].ticker);
    }
  }, [displayEvents, expandedEvent]);

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

  const eventsLoaded = isTodayMode ? todayEvents !== undefined : singleSeriesEvents !== undefined;

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-bold">Sports</h1>

      {/* Series pills with "Today" first */}
      {sortedSeries.length > 0 && (
        <SeriesPillBar
          series={sortedSeries}
          selected={selectedSeries}
          onSelect={selectSeries}
          todayCount={todayEvents?.length ?? null}
        />
      )}

      {/* Game list */}
      {displayEvents.length > 0 && (
        <GameList
          events={displayEvents}
          showLeague={isTodayMode}
          expandedEvent={expandedEvent}
          expandedMarkets={expandedMarkets ?? null}
          positionTickers={positionTickers}
          watchlist={watchlist}
          toggleStar={toggleStar}
          onToggleExpand={toggleExpand}
          onMarketClick={(m) => setSlideOverMarket(m)}
        />
      )}
      {selectedSeries && displayEvents.length === 0 && eventsLoaded && (
        <div className="bg-bg-raised border border-border rounded-lg p-4 text-center text-fg-subtle text-sm">
          {isTodayMode
            ? "No games today"
            : `No upcoming games for ${seriesLabel(selectedSeries)}`}
        </div>
      )}
      {!eventsLoaded && selectedSeries && (
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
  todayCount,
}: {
  series: MonitorSeries[];
  selected: string | null;
  onSelect: (ticker: string) => void;
  todayCount: number | null;
}) {
  return (
    <div className="relative">
      <div className="flex gap-2 overflow-x-auto pb-1 scrollbar-none">
        {/* "Today" pill */}
        <button
          onClick={() => onSelect(TODAY_KEY)}
          className={`shrink-0 px-3 py-1.5 rounded-full text-xs font-medium transition-colors ${
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
  showLeague,
  expandedEvent,
  expandedMarkets,
  positionTickers,
  watchlist,
  toggleStar,
  onToggleExpand,
  onMarketClick,
}: {
  events: MonitorEvent[];
  showLeague: boolean;
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
              {showLeague && <th className="px-4 py-2">League</th>}
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
                showLeague={showLeague}
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
  showLeague,
  isExpanded,
  markets,
  positionTickers,
  watchlist,
  toggleStar,
  onToggle,
  onMarketClick,
}: {
  event: MonitorEvent;
  showLeague: boolean;
  isExpanded: boolean;
  markets: MonitorMarket[] | null;
  positionTickers: Set<string>;
  watchlist: { has: (ticker: string) => boolean };
  toggleStar: (ticker: string, title?: string) => void;
  onToggle: () => void;
  onMarketClick: (m: MonitorMarket) => void;
}) {
  const eventDate = getEventDate(event);
  const precise = eventDate && hasPreciseTime(eventDate);
  const countdown = useCountdown(precise ? eventDate : null);
  const cdColor = countdownColor(precise ? eventDate : null);
  const colSpan = showLeague ? 7 : 6;

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
                <td className="px-4 py-1.5 pl-10" colSpan={showLeague ? 2 : 1}>
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
          <td colSpan={colSpan} className="px-4 py-2 text-center text-fg-subtle text-xs">
            Loading markets...
          </td>
        </tr>
      )}
    </>
  );
}
