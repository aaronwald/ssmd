"use client";

import { useState } from "react";
import type { MonitorMarket, Side, Action } from "@/lib/types";
import { useInstance } from "@/lib/instance-context";
import { CreateOrderFormControlled } from "./create-order-form-controlled";
import { CreateBracketFormControlled } from "./create-bracket-form-controlled";

const EXCHANGE_LABELS: Record<string, string> = {
  kalshi: "Kalshi",
  kraken: "Kraken",
  "kraken-futures": "Kraken Futures",
  polymarket: "Polymarket",
  test: "Test",
};

interface Props {
  market: MonitorMarket;
  onClose: () => void;
  initialSide?: Side;
  initialAction?: Action;
  initialPrice?: string;
}

export function MarketSlideOver({ market, onClose, initialSide, initialAction, initialPrice }: Props) {
  const { instances } = useInstance();
  const [orderMode, setOrderMode] = useState<"single" | "bracket">("single");
  const bid = market.yes_bid ?? market.bid ?? market.best_bid ?? null;
  const ask = market.yes_ask ?? market.ask ?? market.best_ask ?? null;
  const last = market.last ?? (market.price != null ? Number(market.price) : null);
  const fmtPrice = (v: number | null) => v != null ? `$${v.toFixed(2)}` : "—";

  const marketExchange = market.exchange || "kalshi";
  // Filter to compatible instances (same exchange or test)
  const compatible = instances.filter(
    (i) => i.healthy && (i.exchange === marketExchange || i.exchange === "test")
  );
  const [selectedInstance, setSelectedInstance] = useState<string>(
    compatible[0]?.id ?? ""
  );
  const canOrder = compatible.length > 0;

  return (
    <>
      {/* Backdrop */}
      <div className="fixed inset-0 bg-black/30 z-40" onClick={onClose} />

      {/* Panel */}
      <div className="fixed top-0 right-0 h-full w-[400px] max-w-full bg-bg-raised border-l border-border z-50 overflow-y-auto shadow-xl">
        <div className="p-4 border-b border-border flex items-center justify-between">
          <h2 className="text-sm font-bold text-fg truncate" title={market.title}>
            {market.title || market.ticker}
          </h2>
          <button onClick={onClose} className="text-fg-muted hover:text-fg text-lg leading-none">&times;</button>
        </div>

        {/* Market context */}
        <div className="p-4 space-y-3 border-b border-border">
          <div className="flex items-center justify-between">
            <div className="font-mono text-xs text-fg-muted">{market.ticker}</div>
            <span className="inline-block rounded-md px-2 py-0.5 text-xs font-medium bg-accent/15 text-accent">
              {EXCHANGE_LABELS[marketExchange] || marketExchange}
            </span>
          </div>
          <div className="grid grid-cols-3 gap-3 text-center">
            <div>
              <div className="text-xs text-fg-muted">Bid</div>
              <div className="font-mono text-sm text-fg">{fmtPrice(bid)}</div>
            </div>
            <div>
              <div className="text-xs text-fg-muted">Ask</div>
              <div className="font-mono text-sm text-fg">{fmtPrice(ask)}</div>
            </div>
            <div>
              <div className="text-xs text-fg-muted">Last</div>
              <div className="font-mono text-sm text-fg">{fmtPrice(last)}</div>
            </div>
          </div>
          <div className="flex gap-4 text-xs text-fg-muted">
            {market.volume != null && <span>Vol: {market.volume.toLocaleString()}</span>}
            {market.open_interest != null && <span>OI: {market.open_interest.toLocaleString()}</span>}
            {market.status && (
              <span className={market.status === "active" ? "text-green" : "text-fg-subtle"}>
                {market.status}
              </span>
            )}
          </div>
          {market.close_time && (
            <div className="text-xs text-fg-muted">
              Closes: {new Date(market.close_time).toLocaleString()}
            </div>
          )}
        </div>

        {/* Lifecycle timeline */}
        {market.lifecycle_events && market.lifecycle_events.length > 0 && (
          <div className="p-4 border-b border-border">
            <h3 className="text-xs font-medium text-fg-muted mb-2">Lifecycle</h3>
            <div className="space-y-0">
              {market.lifecycle_events.map((ev, i) => (
                <div key={i} className="flex items-start gap-2 relative">
                  {/* Vertical line */}
                  {i < market.lifecycle_events!.length - 1 && (
                    <div className="absolute left-[5px] top-[14px] bottom-0 w-px bg-border" />
                  )}
                  {/* Dot */}
                  <div className={`mt-[5px] w-[11px] h-[11px] rounded-full shrink-0 border-2 ${
                    ev.type === "determined" || ev.type === "settled"
                      ? "border-green bg-green/30"
                      : ev.type === "deactivated"
                        ? "border-red bg-red/30"
                        : ev.type === "activated"
                          ? "border-accent bg-accent/30"
                          : "border-fg-subtle bg-bg-surface"
                  }`} />
                  <div className="pb-3 min-w-0">
                    <div className="text-xs font-medium text-fg">{ev.type}</div>
                    <div className="text-[10px] text-fg-muted">
                      {new Date(ev.ts).toLocaleString("en-US", {
                        month: "short", day: "numeric",
                        hour: "numeric", minute: "2-digit", second: "2-digit",
                        timeZone: "America/New_York",
                      })}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Order form or exchange mismatch message */}
        <div className="p-4">
          {canOrder ? (
            <>
              {/* Single / Bracket toggle */}
              <div className="flex gap-1 mb-3 bg-bg-surface rounded-full p-0.5 border border-border">
                <button
                  onClick={() => setOrderMode("single")}
                  className={`flex-1 px-3 py-1 rounded-full text-xs font-medium transition-colors ${
                    orderMode === "single"
                      ? "bg-accent text-bg"
                      : "text-fg-muted hover:text-fg"
                  }`}
                >
                  Single
                </button>
                <button
                  onClick={() => setOrderMode("bracket")}
                  className={`flex-1 px-3 py-1 rounded-full text-xs font-medium transition-colors ${
                    orderMode === "bracket"
                      ? "bg-accent text-bg"
                      : "text-fg-muted hover:text-fg"
                  }`}
                >
                  Bracket
                </button>
              </div>
              {compatible.length > 1 && (
                <div className="mb-3">
                  <label className="block text-xs text-fg-muted mb-1">Instance</label>
                  <select
                    value={selectedInstance}
                    onChange={(e) => setSelectedInstance(e.target.value)}
                    className="w-full rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm text-fg focus:border-accent focus:outline-none"
                  >
                    {compatible.map((inst) => (
                      <option key={inst.id} value={inst.id}>{inst.id}</option>
                    ))}
                  </select>
                </div>
              )}
              {orderMode === "single" ? (
                <CreateOrderFormControlled
                  ticker={market.ticker}
                  yesBid={bid}
                  yesAsk={ask}
                  last={last}
                  instanceId={selectedInstance}
                  onSuccess={onClose}
                  initialSide={initialSide}
                  initialAction={initialAction}
                  initialPrice={initialPrice}
                />
              ) : (
                <CreateBracketFormControlled
                  ticker={market.ticker}
                  yesBid={bid}
                  yesAsk={ask}
                  last={last}
                  instanceId={selectedInstance}
                  onSuccess={onClose}
                  initialSide={initialSide}
                  initialAction={initialAction}
                  initialPrice={initialPrice}
                />
              )}
            </>
          ) : (
            <div className="rounded-lg border border-border bg-bg-surface p-4 text-center space-y-2">
              <div className="text-sm text-fg-muted">
                No OMS for <span className="font-medium text-fg">{EXCHANGE_LABELS[marketExchange] || marketExchange}</span>
              </div>
              <div className="text-xs text-fg-subtle">
                No healthy {EXCHANGE_LABELS[marketExchange] || marketExchange} instance available.
              </div>
            </div>
          )}
        </div>
      </div>
    </>
  );
}
