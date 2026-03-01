"use client";

import { useState } from "react";
import { v4 as uuidv4 } from "uuid";
import { createOrder } from "@/lib/api";
import type { Side, Action, TimeInForce } from "@/lib/types";
import { useSWRConfig } from "swr";
import { matchInstanceKey } from "@/lib/hooks";

interface Props {
  ticker: string;
  yesBid: number | null;
  yesAsk: number | null;
  last: number | null;
  onSuccess?: () => void;
}

export function CreateOrderFormControlled({ ticker, yesBid, yesAsk, last, onSuccess }: Props) {
  const { mutate } = useSWRConfig();
  const [side, setSide] = useState<Side>("yes");
  const [action, setAction] = useState<Action>("buy");
  const [quantity, setQuantity] = useState("");
  const [price, setPrice] = useState("");
  const [tif, setTif] = useState<TimeInForce>("gtc");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    setSubmitting(true);
    try {
      await createOrder({
        client_order_id: uuidv4(),
        ticker,
        side,
        action,
        quantity,
        price_dollars: price,
        time_in_force: tif,
      });
      setQuantity("");
      setPrice("");
      mutate(matchInstanceKey("orders"));
      onSuccess?.();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create order");
    } finally {
      setSubmitting(false);
    }
  }

  const fmtPrice = (v: number | null) => v != null ? v.toFixed(2) : "—";

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      {/* Ticker (read-only) */}
      <div>
        <label className="block text-xs text-fg-muted mb-1">Ticker</label>
        <input
          type="text"
          value={ticker}
          readOnly
          className="w-full rounded-md border border-border bg-bg px-3 py-1.5 text-sm font-mono text-fg-muted cursor-not-allowed"
        />
      </div>

      {/* Bid/Ask context */}
      <div className="flex gap-4 text-xs text-fg-muted">
        <span>Bid: <span className="font-mono text-fg">${fmtPrice(yesBid)}</span></span>
        <span>Ask: <span className="font-mono text-fg">${fmtPrice(yesAsk)}</span></span>
        <span>Last: <span className="font-mono text-fg">${fmtPrice(last)}</span></span>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="block text-xs text-fg-muted mb-1">Side</label>
          <select
            value={side}
            onChange={(e) => setSide(e.target.value as Side)}
            className="w-full rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm text-fg focus:border-accent focus:outline-none"
          >
            <option value="yes">Yes</option>
            <option value="no">No</option>
          </select>
        </div>
        <div>
          <label className="block text-xs text-fg-muted mb-1">Action</label>
          <select
            value={action}
            onChange={(e) => setAction(e.target.value as Action)}
            className="w-full rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm text-fg focus:border-accent focus:outline-none"
          >
            <option value="buy">Buy</option>
            <option value="sell">Sell</option>
          </select>
        </div>
      </div>

      <div>
        <label className="block text-xs text-fg-muted mb-1">Quantity</label>
        <input
          type="text"
          value={quantity}
          onChange={(e) => setQuantity(e.target.value)}
          required
          className="w-full rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm font-mono text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none"
          placeholder="10"
        />
      </div>

      <div>
        <label className="block text-xs text-fg-muted mb-1">Price ($)</label>
        <input
          type="text"
          value={price}
          onChange={(e) => setPrice(e.target.value)}
          required
          className="w-full rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm font-mono text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none"
          placeholder="0.55"
        />
        {/* Quick-fill price buttons */}
        <div className="flex gap-2 mt-1.5">
          {yesBid != null && (
            <button type="button" onClick={() => setPrice(yesBid.toFixed(2))} className="text-xs text-accent hover:underline">
              Bid ${yesBid.toFixed(2)}
            </button>
          )}
          {yesAsk != null && (
            <button type="button" onClick={() => setPrice(yesAsk.toFixed(2))} className="text-xs text-accent hover:underline">
              Ask ${yesAsk.toFixed(2)}
            </button>
          )}
          {yesBid != null && yesAsk != null && (
            <button type="button" onClick={() => setPrice(((yesBid + yesAsk) / 2).toFixed(2))} className="text-xs text-accent hover:underline">
              Mid ${((yesBid + yesAsk) / 2).toFixed(2)}
            </button>
          )}
          {last != null && (
            <button type="button" onClick={() => setPrice(last.toFixed(2))} className="text-xs text-accent hover:underline">
              Last ${last.toFixed(2)}
            </button>
          )}
        </div>
      </div>

      <div>
        <label className="block text-xs text-fg-muted mb-1">TIF</label>
        <select
          value={tif}
          onChange={(e) => setTif(e.target.value as TimeInForce)}
          className="w-full rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm text-fg focus:border-accent focus:outline-none"
        >
          <option value="gtc">GTC</option>
          <option value="ioc">IOC</option>
        </select>
      </div>

      {error && <p className="text-xs text-red">{error}</p>}
      <button
        type="submit"
        disabled={submitting}
        className="w-full rounded-md bg-accent px-4 py-1.5 text-sm font-medium text-fg hover:bg-accent-hover transition-colors disabled:opacity-50"
      >
        {submitting ? "Submitting..." : "Submit Order"}
      </button>
    </form>
  );
}
