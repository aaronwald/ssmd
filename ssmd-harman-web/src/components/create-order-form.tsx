"use client";

import { useState } from "react";
import { v4 as uuidv4 } from "uuid";
import { createOrder } from "@/lib/api";
import type { Side, Action, TimeInForce } from "@/lib/types";
import { useSWRConfig } from "swr";
import { TickerInput } from "./ticker-input";

export function CreateOrderForm() {
  const { mutate } = useSWRConfig();
  const [ticker, setTicker] = useState("");
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
      setTicker("");
      setQuantity("");
      setPrice("");
      mutate((key: string) => typeof key === "string" && key.startsWith("orders"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create order");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <form onSubmit={handleSubmit} className="bg-bg-raised border border-border rounded-lg p-4 space-y-4">
      <h3 className="text-sm font-medium text-fg">Create Order</h3>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
        <div>
          <label className="block text-xs text-fg-muted mb-1">Ticker</label>
          <TickerInput
            value={ticker}
            onChange={setTicker}
            className="w-full rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm font-mono text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none"
          />
        </div>
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
      </div>
      {error && <p className="text-xs text-red">{error}</p>}
      <button
        type="submit"
        disabled={submitting}
        className="rounded-md bg-accent px-4 py-1.5 text-sm font-medium text-fg hover:bg-accent-hover transition-colors disabled:opacity-50"
      >
        {submitting ? "Submitting..." : "Submit Order"}
      </button>
    </form>
  );
}
