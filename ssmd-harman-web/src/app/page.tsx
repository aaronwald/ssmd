"use client";

import { useMemo, useState } from "react";
import Link from "next/link";
import { StatusDot } from "@/components/status-dot";
import { RiskGauge } from "@/components/risk-gauge";
import { SnapAgeDot } from "@/components/snap-age-dot";
import { useHealth, usePositions, useRisk, useSnapMap, useInfo } from "@/lib/hooks";
import type { ExchangePosition, LocalPosition, NormalizedSnapshot } from "@/lib/types";

export default function Dashboard() {
  const { data: health } = useHealth();
  const { data: positions } = usePositions();
  const { data: risk } = useRisk();
  const { data: info } = useInfo();
  const feed = info?.exchange ?? "kalshi";
  const { data: snapMap, error: snapError } = useSnapMap(feed);
  const [hideZero, setHideZero] = useState(false);

  const filteredExchange = useMemo(() => {
    if (!positions) return [];
    if (!hideZero) return positions.exchange;
    return positions.exchange.filter((p: ExchangePosition) => parseFloat(p.quantity) !== 0);
  }, [positions, hideZero]);

  const filteredLocal = useMemo(() => {
    if (!positions) return [];
    if (!hideZero) return positions.local;
    return positions.local.filter((p: LocalPosition) => parseFloat(p.net_quantity) !== 0);
  }, [positions, hideZero]);

  const snapFor = (ticker: string): NormalizedSnapshot | undefined =>
    snapMap?.get(ticker);

  const healthStatus = health
    ? health.status === "healthy"
      ? "green"
      : "red"
    : "yellow";

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-bold">Dashboard</h1>

      {/* Health + Risk row */}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <div className="bg-bg-raised border border-border rounded-lg p-4 space-y-3">
          <h2 className="text-sm font-medium text-fg-muted">Health</h2>
          <StatusDot status={healthStatus} />
          {health && (
            <div className="text-xs text-fg-muted space-y-1">
              <div>Session: <span className="font-mono text-fg">{health.session_state}</span></div>
              <div>Uptime: <span className="font-mono text-fg">{Math.floor(health.uptime_seconds)}s</span></div>
            </div>
          )}
        </div>

        <div className="bg-bg-raised border border-border rounded-lg p-4 space-y-3">
          <h2 className="text-sm font-medium text-fg-muted">Risk</h2>
          {risk ? <RiskGauge risk={risk} /> : <span className="text-xs text-fg-subtle">Loading...</span>}
        </div>
      </div>

      {/* Snap error banner */}
      {snapError && (
        <div className="bg-red/10 border border-red/30 rounded-lg p-3 flex items-center gap-2">
          <span className="inline-block w-2 h-2 rounded-full bg-red" />
          <span className="text-sm text-red">Snap feed unavailable — live prices may be stale</span>
        </div>
      )}

      {/* Positions */}
      <div className="bg-bg-raised border border-border rounded-lg p-4 space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-medium text-fg-muted">Positions</h2>
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
        {positions && (filteredExchange.length > 0 || filteredLocal.length > 0) ? (
          <div className="space-y-4">
            {/* Exchange positions */}
            {filteredExchange.length > 0 && (
            <div>
              <h3 className="text-xs font-medium text-fg-muted mb-2 capitalize">{info?.exchange ?? "Exchange"} Positions</h3>
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="text-left text-xs text-fg-muted border-b border-border">
                      <th className="pb-2 pr-4">Ticker</th>
                      <th className="pb-2 pr-4">Side</th>
                      <th className="pb-2 pr-4 text-right">Qty</th>
                      <th className="pb-2 pr-4 text-right">Mkt Value</th>
                      <th className="pb-2 pr-4 text-right">Yes Bid</th>
                      <th className="pb-2 pr-4 text-right">Yes Ask</th>
                      <th className="pb-2 text-right">Last</th>
                    </tr>
                  </thead>
                  <tbody>
                    {filteredExchange.map((p: ExchangePosition) => {
                      const snap = snapFor(p.ticker);
                      return (
                        <tr key={p.ticker} className="border-b border-border-subtle">
                          <td className="py-2 pr-4 font-mono">
                            <Link href={`/markets?q=${encodeURIComponent(p.ticker)}`} className="text-accent hover:underline">{p.ticker}</Link>
                            <SnapAgeDot snapAt={snap?.snapAt ?? null} />
                          </td>
                          <td className="py-2 pr-4 uppercase">{p.side}</td>
                          <td className="py-2 pr-4 font-mono text-right">{p.quantity}</td>
                          <td className="py-2 pr-4 font-mono text-right">${p.market_value_dollars}</td>
                          <td className="py-2 pr-4 font-mono text-right text-fg-muted">{snap?.yesBid != null ? snap.yesBid.toFixed(2) : "—"}</td>
                          <td className="py-2 pr-4 font-mono text-right text-fg-muted">{snap?.yesAsk != null ? snap.yesAsk.toFixed(2) : "—"}</td>
                          <td className="py-2 font-mono text-right text-fg-muted">{snap?.last != null ? snap.last.toFixed(2) : "—"}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </div>
            )}
            {/* Local positions */}
            {filteredLocal.length > 0 && (
              <div>
                <h3 className="text-xs font-medium text-fg-muted mb-2">Local</h3>
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="text-left text-xs text-fg-muted border-b border-border">
                        <th className="pb-2 pr-4">Ticker</th>
                        <th className="pb-2 pr-4 text-right">Net Qty</th>
                        <th className="pb-2 pr-4 text-right">Buy Filled</th>
                        <th className="pb-2 pr-4 text-right">Sell Filled</th>
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
                            <td className="py-2 pr-4 font-mono text-right text-fg-muted">{snap?.last != null ? snap.last.toFixed(2) : "—"}</td>
                            <td className="py-2 font-mono text-right text-fg-muted">{mktVal != null ? `$${mktVal.toFixed(2)}` : "—"}</td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              </div>
            )}
          </div>
        ) : (
          <p className="text-xs text-fg-subtle">No positions</p>
        )}
      </div>
    </div>
  );
}
