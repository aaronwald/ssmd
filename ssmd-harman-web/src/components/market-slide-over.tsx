"use client";

import type { MonitorMarket } from "@/lib/types";
import { CreateOrderFormControlled } from "./create-order-form-controlled";

interface Props {
  market: MonitorMarket;
  onClose: () => void;
}

export function MarketSlideOver({ market, onClose }: Props) {
  const bid = market.yes_bid ?? market.bid ?? market.best_bid ?? null;
  const ask = market.yes_ask ?? market.ask ?? market.best_ask ?? null;
  const last = market.last ?? (market.price != null ? Number(market.price) : null);
  const fmtPrice = (v: number | null) => v != null ? `$${v.toFixed(2)}` : "—";

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
          <div className="font-mono text-xs text-fg-muted">{market.ticker}</div>
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

        {/* Order form */}
        <div className="p-4">
          <h3 className="text-sm font-medium text-fg mb-3">Place Order</h3>
          <CreateOrderFormControlled
            ticker={market.ticker}
            yesBid={bid}
            yesAsk={ask}
            last={last}
            onSuccess={onClose}
          />
        </div>
      </div>
    </>
  );
}
