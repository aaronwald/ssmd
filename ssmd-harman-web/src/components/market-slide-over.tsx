"use client";

import type { MonitorMarket } from "@/lib/types";
import { useInfo } from "@/lib/hooks";
import { CreateOrderFormControlled } from "./create-order-form-controlled";

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
}

export function MarketSlideOver({ market, onClose }: Props) {
  const { data: info } = useInfo();
  const bid = market.yes_bid ?? market.bid ?? market.best_bid ?? null;
  const ask = market.yes_ask ?? market.ask ?? market.best_ask ?? null;
  const last = market.last ?? (market.price != null ? Number(market.price) : null);
  const fmtPrice = (v: number | null) => v != null ? `$${v.toFixed(2)}` : "—";

  const marketExchange = market.exchange || "kalshi";
  const instanceExchange = info?.exchange || "kalshi";
  // Allow ordering if exchanges match, or if instance is "test" (test-exchange accepts any ticker)
  const canOrder = instanceExchange === marketExchange || instanceExchange === "test";

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

        {/* Order form or exchange mismatch message */}
        <div className="p-4">
          {canOrder ? (
            <>
              <h3 className="text-sm font-medium text-fg mb-3">Place Order</h3>
              <CreateOrderFormControlled
                ticker={market.ticker}
                yesBid={bid}
                yesAsk={ask}
                last={last}
                onSuccess={onClose}
              />
            </>
          ) : (
            <div className="rounded-lg border border-border bg-bg-surface p-4 text-center space-y-2">
              <div className="text-sm text-fg-muted">
                No OMS for <span className="font-medium text-fg">{EXCHANGE_LABELS[marketExchange] || marketExchange}</span>
              </div>
              <div className="text-xs text-fg-subtle">
                This instance routes to {EXCHANGE_LABELS[instanceExchange] || instanceExchange}. Switch to a {EXCHANGE_LABELS[marketExchange] || marketExchange} instance to trade this market.
              </div>
            </div>
          )}
        </div>
      </div>
    </>
  );
}
