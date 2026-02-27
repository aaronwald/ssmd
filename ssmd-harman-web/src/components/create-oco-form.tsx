"use client";

import { useState } from "react";
import { v4 as uuidv4 } from "uuid";
import { createOco } from "@/lib/api";
import type { Side, Action, TimeInForce } from "@/lib/types";
import { useSWRConfig } from "swr";
import { TickerInput } from "./ticker-input";

export function CreateOcoForm() {
  const { mutate } = useSWRConfig();
  const [ticker, setTicker] = useState("");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const [l1Side, setL1Side] = useState<Side>("yes");
  const [l1Action, setL1Action] = useState<Action>("buy");
  const [l1Qty, setL1Qty] = useState("");
  const [l1Price, setL1Price] = useState("");
  const [l1Tif, setL1Tif] = useState<TimeInForce>("gtc");

  const [l2Side, setL2Side] = useState<Side>("no");
  const [l2Action, setL2Action] = useState<Action>("buy");
  const [l2Qty, setL2Qty] = useState("");
  const [l2Price, setL2Price] = useState("");
  const [l2Tif, setL2Tif] = useState<TimeInForce>("gtc");

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    setSubmitting(true);
    try {
      await createOco({
        leg1: { client_order_id: uuidv4(), ticker, side: l1Side, action: l1Action, quantity: l1Qty, price_dollars: l1Price, time_in_force: l1Tif },
        leg2: { client_order_id: uuidv4(), ticker, side: l2Side, action: l2Action, quantity: l2Qty, price_dollars: l2Price, time_in_force: l2Tif },
      });
      mutate((key: string) => typeof key === "string" && key.startsWith("groups"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create OCO");
    } finally {
      setSubmitting(false);
    }
  }

  const inputCls = "rounded border border-border bg-bg-surface px-2 py-1 text-xs font-mono text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none";
  const selectCls = "rounded border border-border bg-bg-surface px-2 py-1 text-xs text-fg focus:border-accent focus:outline-none";

  return (
    <form onSubmit={handleSubmit} className="bg-bg-raised border border-border rounded-lg p-4 space-y-4">
      <h3 className="text-sm font-medium text-fg">Create OCO</h3>
      <div>
        <label className="block text-xs text-fg-muted mb-1">Ticker (shared)</label>
        <TickerInput
          value={ticker}
          onChange={setTicker}
          className="w-full max-w-xs rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm font-mono text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none"
        />
      </div>
      {/* Leg 1 */}
      <div className="space-y-2">
        <span className="text-xs font-medium text-fg-muted uppercase">Leg 1</span>
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-5">
          <select value={l1Side} onChange={(e) => setL1Side(e.target.value as Side)} className={selectCls}><option value="yes">Yes</option><option value="no">No</option></select>
          <select value={l1Action} onChange={(e) => setL1Action(e.target.value as Action)} className={selectCls}><option value="buy">Buy</option><option value="sell">Sell</option></select>
          <input type="text" value={l1Qty} onChange={(e) => setL1Qty(e.target.value)} placeholder="Qty" className={inputCls} />
          <input type="text" value={l1Price} onChange={(e) => setL1Price(e.target.value)} placeholder="Price" className={inputCls} />
          <select value={l1Tif} onChange={(e) => setL1Tif(e.target.value as TimeInForce)} className={selectCls}><option value="gtc">GTC</option><option value="ioc">IOC</option></select>
        </div>
      </div>
      {/* Leg 2 */}
      <div className="space-y-2">
        <span className="text-xs font-medium text-fg-muted uppercase">Leg 2</span>
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-5">
          <select value={l2Side} onChange={(e) => setL2Side(e.target.value as Side)} className={selectCls}><option value="yes">Yes</option><option value="no">No</option></select>
          <select value={l2Action} onChange={(e) => setL2Action(e.target.value as Action)} className={selectCls}><option value="buy">Buy</option><option value="sell">Sell</option></select>
          <input type="text" value={l2Qty} onChange={(e) => setL2Qty(e.target.value)} placeholder="Qty" className={inputCls} />
          <input type="text" value={l2Price} onChange={(e) => setL2Price(e.target.value)} placeholder="Price" className={inputCls} />
          <select value={l2Tif} onChange={(e) => setL2Tif(e.target.value as TimeInForce)} className={selectCls}><option value="gtc">GTC</option><option value="ioc">IOC</option></select>
        </div>
      </div>
      {error && <p className="text-xs text-red">{error}</p>}
      <button type="submit" disabled={submitting} className="rounded-md bg-accent px-4 py-1.5 text-sm font-medium text-fg hover:bg-accent-hover transition-colors disabled:opacity-50">
        {submitting ? "Submitting..." : "Create OCO"}
      </button>
    </form>
  );
}
