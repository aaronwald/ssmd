"use client";

import useSWR from "swr";
import { useEffect, useState } from "react";
import { getSeries, getEvents, getMarkets } from "@/lib/api";
import type { MonitorEvent, MonitorMarket } from "@/lib/types";

// 15-minute crypto markets are momentum contracts ("is <coin> up in the next 15 min?")
// with one market per event and no fixed strike. Every coin's contract rolls on the same
// 15-minute boundary, so we show one compact live row per coin with a single shared
// countdown. Prices come from the monitor markets endpoint (snap merged server-side) —
// the same priced path the strike table uses (NOT useSnapMap, which is instance-scoped).
const SERIES_SUFFIX = "15M";
const REFRESH_MS = 5000;

interface PanelRow {
  coin: string;
  series: string;
  eventTicker: string;
  volume: number | null;
  yesBid: number | null;
  yesAsk: number | null;
  last: number | null;
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
 * Every API response is treated as untrusted: non-arrays and missing fields are
 * coerced to empty / null rather than allowed to throw mid-render.
 */
async function fetch15mRows(): Promise<PanelRow[]> {
  const allSeries = await getSeries("Crypto");
  if (!Array.isArray(allSeries) || allSeries.length === 0) return [];
  const series15m = allSeries.filter((s) => typeof s?.ticker === "string" && s.ticker.endsWith(SERIES_SUFFIX));
  if (series15m.length === 0) return [];

  const withEvents = await Promise.all(
    series15m.map(async (s) => {
      const events = await getEvents(s.ticker);
      return { series: s.ticker, events: Array.isArray(events) ? events : [] };
    }),
  );

  const now = Date.now();
  const liveEvents = withEvents
    .map(({ series, events }) => {
      const live = events
        .filter((e) => e?.status === "active")
        .map((e) => ({ e, t: eventCloseMs(e) }))
        .filter((x): x is { e: MonitorEvent; t: number } => x.t != null && x.t > now)
        .sort((a, b) => a.t - b.t)[0];
      return live ? { series, event: live.e, closeMs: live.t } : null;
    })
    .filter((x): x is { series: string; event: MonitorEvent; closeMs: number } => x != null);

  const rows = await Promise.all(
    liveEvents.map(async ({ series, event, closeMs }) => {
      const markets = await getMarkets(event.ticker);
      const m: MonitorMarket | undefined = Array.isArray(markets) ? markets[0] : undefined;
      if (!m || typeof m.ticker !== "string") return null;
      return {
        coin: coinFromSeries(series),
        series,
        eventTicker: event.ticker,
        volume: m.volume ?? null,
        yesBid: m.yes_bid ?? null,
        yesAsk: m.yes_ask ?? null,
        last: m.last ?? null,
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

/** Mid of yes bid/ask, falling back to last. Result is a 0–1 probability. */
function yesProbability(row: PanelRow): number | null {
  if (row.yesBid != null && row.yesAsk != null) return (row.yesBid + row.yesAsk) / 2;
  return row.last;
}

/** Compact volume, e.g. 386957 -> "387k", 1_240_000 -> "1.2M". */
function fmtVol(v: number | null): string {
  if (v == null) return "—";
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${Math.round(v / 1_000)}k`;
  return Math.round(v).toLocaleString();
}

export function FifteenMinPanel({
  onSelect,
}: {
  onSelect: (series: string, event: string) => void;
}) {
  const { data: rows, error } = useSWR("data-15m-rows", fetch15mRows, {
    refreshInterval: REFRESH_MS,
    keepPreviousData: true,
  });

  // All coins roll together → one shared countdown to the soonest close.
  const closeMs = rows && rows.length > 0 ? Math.min(...rows.map((r) => r.closeMs)) : null;
  const countdown = useCountdown(closeMs);

  return (
    <section aria-labelledby="fifteen-min-heading" className="rounded-lg border border-border bg-bg-raised">
      <header className="flex items-baseline justify-between border-b border-border px-4 py-3">
        <h2 id="fifteen-min-heading" className="text-base font-semibold text-fg">
          15-Minute <span className="font-normal text-fg-subtle">· up in next 15 min?</span>
        </h2>
        <span className="font-mono text-sm text-red">⏱ {countdown}</span>
      </header>

      {error && (
        <div className="px-4 py-6 text-center text-sm text-red">Failed to load 15-minute markets.</div>
      )}
      {!error && !rows && (
        <div className="px-4 py-6 text-center text-sm text-fg-subtle">Loading…</div>
      )}
      {!error && rows && rows.length === 0 && (
        <div className="px-4 py-6 text-center text-sm text-fg-subtle">No active 15-minute markets.</div>
      )}

      {rows && rows.length > 0 && (
        <ul className="divide-y divide-border">
          {rows.map((r) => {
            const yes = yesProbability(r);
            return (
              <li key={r.series}>
                <button
                  onClick={() => onSelect(r.series, r.eventTicker)}
                  className="flex w-full items-center gap-4 px-4 py-2.5 text-left transition-colors hover:bg-bg"
                >
                  <span className="w-16 shrink-0 font-medium text-fg">{r.coin}</span>
                  <span className="w-28 shrink-0 font-mono text-sm">
                    <span className="text-fg-subtle">Yes </span>
                    <span className={yes == null ? "text-fg-subtle" : yes >= 0.5 ? "text-green" : "text-fg"}>
                      {yes == null ? "—" : yes.toFixed(2)}
                    </span>
                  </span>
                  <span className="ml-auto font-mono text-xs text-fg-subtle">vol {fmtVol(r.volume)}</span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
