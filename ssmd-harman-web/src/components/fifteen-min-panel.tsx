"use client";

import useSWR from "swr";
import { useEffect, useState } from "react";
import { getSeries, getEvents, getMarkets } from "@/lib/api";
import { useSnapMap } from "@/lib/hooks";
import type { MonitorEvent } from "@/lib/types";

// 15-minute crypto markets are momentum contracts ("is <coin> up in the next 15 min?")
// with one market per event and no fixed strike. Every coin's contract rolls on the same
// 15-minute boundary, so we show one compact live row per coin with a single shared
// countdown. Metadata (which event is currently live) refreshes slowly; prices come from
// snap on the fast path.
const SERIES_SUFFIX = "15M";
const META_REFRESH_MS = 15000; // catch the 15-min roll within ~15s
const SNAP_FEED = "kalshi";

interface PanelRow {
  coin: string;
  series: string;
  eventTicker: string;
  marketTicker: string;
  volume: number | null;
  closeMs: number;
}

/** Best available close time for an event (ms epoch), or null. */
function eventCloseMs(e: MonitorEvent): number | null {
  const s = e.strike_date ?? e.expected_expiration_time ?? null;
  if (!s) return null;
  const t = new Date(s).getTime();
  return Number.isNaN(t) ? null : t;
}

/** KXBTC15M -> BTC */
function coinFromSeries(series: string): string {
  return series.replace(/^KX/, "").replace(new RegExp(`${SERIES_SUFFIX}$`), "");
}

/**
 * Fetch the current live 15-minute market for every *15M crypto series.
 * Per series, the live market is the active event with the soonest future close.
 */
async function fetch15mRows(): Promise<PanelRow[]> {
  const allSeries = await getSeries("Crypto");
  const series15m = allSeries.filter((s) => s.ticker.endsWith(SERIES_SUFFIX));
  if (series15m.length === 0) return [];

  const withEvents = await Promise.all(
    series15m.map(async (s) => ({ series: s.ticker, events: await getEvents(s.ticker) })),
  );

  const now = Date.now();
  const liveEvents = withEvents
    .map(({ series, events }) => {
      const live = events
        .filter((e) => e.status === "active")
        .map((e) => ({ e, t: eventCloseMs(e) }))
        .filter((x): x is { e: MonitorEvent; t: number } => x.t != null && x.t > now)
        .sort((a, b) => a.t - b.t)[0];
      return live ? { series, event: live.e, closeMs: live.t } : null;
    })
    .filter((x): x is { series: string; event: MonitorEvent; closeMs: number } => x != null);

  const rows = await Promise.all(
    liveEvents.map(async ({ series, event, closeMs }) => {
      const markets = await getMarkets(event.ticker);
      const m = markets[0];
      if (!m) return null;
      return {
        coin: coinFromSeries(series),
        series,
        eventTicker: event.ticker,
        marketTicker: m.ticker,
        volume: m.volume,
        closeMs,
      } satisfies PanelRow;
    }),
  );

  return rows
    .filter((r): r is PanelRow => r != null)
    .sort((a, b) => (b.volume ?? 0) - (a.volume ?? 0));
}

/** Live mm:ss countdown to an epoch-ms target. */
function useCountdown(targetMs: number | null): string {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (targetMs == null) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [targetMs]);
  if (targetMs == null) return "";
  const diff = targetMs - now;
  if (diff <= 0) return "rolling…";
  const mins = Math.floor(diff / 60000);
  const secs = Math.floor((diff % 60000) / 1000);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

/** Mid of yes bid/ask, falling back to last. */
function yesProbability(snap: { yesBid: number | null; yesAsk: number | null; last: number | null } | undefined): number | null {
  if (!snap) return null;
  if (snap.yesBid != null && snap.yesAsk != null) return (snap.yesBid + snap.yesAsk) / 2;
  return snap.last;
}

export function FifteenMinPanel({
  onSelect,
}: {
  onSelect: (series: string, event: string) => void;
}) {
  const { data: rows } = useSWR("data-15m-rows", fetch15mRows, {
    refreshInterval: META_REFRESH_MS,
    keepPreviousData: true,
  });

  const tickers = rows?.map((r) => r.marketTicker);
  const { data: snap } = useSnapMap(SNAP_FEED, tickers);

  // All coins roll together → one shared countdown to the soonest close.
  const closeMs = rows && rows.length > 0 ? Math.min(...rows.map((r) => r.closeMs)) : null;
  const countdown = useCountdown(closeMs);

  if (!rows || rows.length === 0) return null;

  return (
    <section aria-labelledby="fifteen-min-heading" className="rounded-lg border border-border bg-bg-raised">
      <header className="flex items-baseline justify-between border-b border-border px-4 py-2">
        <h2 id="fifteen-min-heading" className="text-sm font-semibold text-fg">
          15-Minute <span className="font-normal text-fg-subtle">· up in next 15 min?</span>
        </h2>
        <span className="font-mono text-xs text-red">⏱ {countdown}</span>
      </header>
      <ul className="divide-y divide-border">
        {rows.map((r) => {
          const yes = yesProbability(snap?.get(r.marketTicker));
          return (
            <li key={r.series}>
              <button
                onClick={() => onSelect(r.series, r.eventTicker)}
                className="flex w-full items-center gap-4 px-4 py-2 text-left transition-colors hover:bg-bg"
              >
                <span className="w-16 shrink-0 font-medium text-fg">{r.coin}</span>
                <span className="w-24 shrink-0 font-mono text-sm">
                  <span className="text-fg-subtle">Yes </span>
                  <span className={yes == null ? "text-fg-subtle" : yes >= 0.5 ? "text-green" : "text-fg"}>
                    {yes == null ? "—" : yes.toFixed(2)}
                  </span>
                </span>
                <span className="ml-auto font-mono text-xs text-fg-subtle">
                  vol {r.volume != null ? r.volume.toLocaleString() : "—"}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
