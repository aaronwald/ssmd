"use client";

import { useMemo, useState } from "react";
import Link from "next/link";
import { SnapAgeDot } from "@/components/snap-age-dot";
import { usePositions, useSnapMap, useInfo } from "@/lib/hooks";
import type { LocalPosition, NormalizedSnapshot } from "@/lib/types";

export default function Dashboard() {
  const { data: positions } = usePositions();
  const { data: info } = useInfo();
  const feed = info?.exchange ?? "kalshi";
  const [hideZero, setHideZero] = useState(false);

  // Collect all position tickers for targeted snap lookup
  const positionTickers = useMemo(() => {
    if (!positions) return undefined;
    const tickers = new Set<string>();
    for (const p of positions.positions) tickers.add(p.ticker);
    return tickers.size > 0 ? Array.from(tickers) : undefined;
  }, [positions]);

  const { data: snapMap, error: snapError } = useSnapMap(feed, positionTickers);

  const filteredLocal = useMemo(() => {
    if (!positions) return [];
    if (!hideZero) return positions.positions;
    return positions.positions.filter((p: LocalPosition) => parseFloat(p.net_quantity) !== 0);
  }, [positions, hideZero]);

  const snapFor = (ticker: string): NormalizedSnapshot | undefined =>
    snapMap?.get(ticker);

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-bold">Positions</h1>

      {/* Snap error banner */}
      {snapError && (
        <div className="bg-red/10 border border-red/30 rounded-lg p-3 flex items-center gap-2">
          <span className="inline-block w-2 h-2 rounded-full bg-red" />
          <span className="text-sm text-red">Snap feed unavailable — live prices may be stale</span>
        </div>
      )}

      {/* Positions (from fills — authoritative) */}
      <div className="bg-bg-raised border border-border rounded-lg p-4 space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-medium text-fg-muted">From Fills</h2>
          <label className="flex items-center gap-2 text-xs text-fg-muted cursor-pointer select-none">
            <input
              type="checkbox"
              checked={hideZero}
              onChange={(e) => setHideZero(e.target.checked)}
              className="rounded border-border bg-bg accent-accent"
            />
            Hide zero
          </label>
        </div>
        {positions && filteredLocal.length > 0 ? (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-xs text-fg-muted border-b border-border">
                  <th className="pb-2 pr-4">Ticker</th>
                  <th className="pb-2 pr-4 text-right">Position</th>
                  <th className="pb-2 pr-4 text-right">Buy Filled</th>
                  <th className="pb-2 pr-4 text-right">Sell Filled</th>
                  <th className="pb-2 pr-4 text-right">Bid</th>
                  <th className="pb-2 pr-4 text-right">Ask</th>
                  <th className="pb-2 pr-4 text-right">Last</th>
                  <th className="pb-2 text-right">Mkt Value</th>
                </tr>
              </thead>
              <tbody>
                {filteredLocal.map((p: LocalPosition) => {
                  const snap = snapFor(p.ticker);
                  const netQty = parseFloat(p.net_quantity);
                  const mktVal = snap?.last != null ? netQty * snap.last : null;
                  return (
                    <tr key={p.ticker} className="border-b border-border-subtle">
                      <td className="py-2 pr-4 font-mono">
                        <Link href={`/markets?q=${encodeURIComponent(p.ticker)}`} className="text-accent hover:underline">{p.ticker}</Link>
                        <SnapAgeDot snapAt={snap?.snapAt ?? null} />
                      </td>
                      <td className={`py-2 pr-4 font-mono text-right ${netQty > 0 ? "text-green" : netQty < 0 ? "text-red" : "text-fg-subtle"}`}>{p.net_quantity}</td>
                      <td className="py-2 pr-4 font-mono text-right">{p.buy_filled}</td>
                      <td className="py-2 pr-4 font-mono text-right">{p.sell_filled}</td>
                      <td className="py-2 pr-4 font-mono text-right text-fg-muted">{snap?.yesBid != null ? snap.yesBid.toFixed(2) : "—"}</td>
                      <td className="py-2 pr-4 font-mono text-right text-fg-muted">{snap?.yesAsk != null ? snap.yesAsk.toFixed(2) : "—"}</td>
                      <td className="py-2 pr-4 font-mono text-right text-fg-muted">{snap?.last != null ? snap.last.toFixed(2) : "—"}</td>
                      <td className="py-2 font-mono text-right text-fg-muted">{mktVal != null ? `$${mktVal.toFixed(2)}` : "—"}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="text-xs text-fg-subtle">No positions</p>
        )}
      </div>
    </div>
  );
}
