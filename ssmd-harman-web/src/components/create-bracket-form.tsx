"use client";

import { useState } from "react";
import { v4 as uuidv4 } from "uuid";
import { createBracket } from "@/lib/api";
import type { Side, Action, TimeInForce } from "@/lib/types";
import { useSWRConfig } from "swr";
import { matchInstanceKey } from "@/lib/hooks";
import { TickerInput } from "./ticker-input";

function LegFields({
  label,
  side,
  setSide,
  action,
  setAction,
  quantity,
  setQuantity,
  price,
  setPrice,
  tif,
  setTif,
}: {
  label: string;
  side: Side;
  setSide: (v: Side) => void;
  action: Action;
  setAction: (v: Action) => void;
  quantity: string;
  setQuantity: (v: string) => void;
  price: string;
  setPrice: (v: string) => void;
  tif: TimeInForce;
  setTif: (v: TimeInForce) => void;
}) {
  return (
    <div className="space-y-2">
      <span className="text-xs font-medium text-fg-muted uppercase">{label}</span>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        <select value={side} onChange={(e) => setSide(e.target.value as Side)} className="rounded border border-border bg-bg-surface px-2 py-1 text-xs text-fg focus:border-accent focus:outline-none">
          <option value="yes">Yes</option>
          <option value="no">No</option>
        </select>
        <select value={action} onChange={(e) => setAction(e.target.value as Action)} className="rounded border border-border bg-bg-surface px-2 py-1 text-xs text-fg focus:border-accent focus:outline-none">
          <option value="buy">Buy</option>
          <option value="sell">Sell</option>
        </select>
        <input type="text" value={quantity} onChange={(e) => setQuantity(e.target.value)} placeholder="Qty" className="rounded border border-border bg-bg-surface px-2 py-1 text-xs font-mono text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none" />
        <input type="text" value={price} onChange={(e) => setPrice(e.target.value)} placeholder="Price" className="rounded border border-border bg-bg-surface px-2 py-1 text-xs font-mono text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none" />
        <select value={tif} onChange={(e) => setTif(e.target.value as TimeInForce)} className="rounded border border-border bg-bg-surface px-2 py-1 text-xs text-fg focus:border-accent focus:outline-none">
          <option value="gtc">GTC</option>
          <option value="ioc">IOC</option>
        </select>
      </div>
    </div>
  );
}

export function CreateBracketForm() {
  const { mutate } = useSWRConfig();
  const [ticker, setTicker] = useState("");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);

  // Entry leg
  const [eSide, setESide] = useState<Side>("yes");
  const [eAction, setEAction] = useState<Action>("buy");
  const [eQty, setEQty] = useState("");
  const [ePrice, setEPrice] = useState("");
  const [eTif, setETif] = useState<TimeInForce>("gtc");

  // Take profit leg
  const [tpSide, setTpSide] = useState<Side>("yes");
  const [tpAction, setTpAction] = useState<Action>("sell");
  const [tpQty, setTpQty] = useState("");
  const [tpPrice, setTpPrice] = useState("");
  const [tpTif, setTpTif] = useState<TimeInForce>("gtc");

  // Stop loss leg
  const [slSide, setSlSide] = useState<Side>("no");
  const [slAction, setSlAction] = useState<Action>("sell");
  const [slQty, setSlQty] = useState("");
  const [slPrice, setSlPrice] = useState("");
  const [slTif, setSlTif] = useState<TimeInForce>("gtc");

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    setSubmitting(true);
    try {
      await createBracket({
        entry: { client_order_id: uuidv4(), ticker, side: eSide, action: eAction, quantity: eQty, price_dollars: ePrice, time_in_force: eTif },
        take_profit: { client_order_id: uuidv4(), ticker, side: tpSide, action: tpAction, quantity: tpQty, price_dollars: tpPrice, time_in_force: tpTif },
        stop_loss: { client_order_id: uuidv4(), ticker, side: slSide, action: slAction, quantity: slQty, price_dollars: slPrice, time_in_force: slTif },
      });
      mutate(matchInstanceKey("groups"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create bracket");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <form onSubmit={handleSubmit} className="bg-bg-raised border border-border rounded-lg p-4 space-y-4">
      <h3 className="text-sm font-medium text-fg">Create Bracket</h3>
      <div>
        <label className="block text-xs text-fg-muted mb-1">Ticker (shared)</label>
        <TickerInput
          value={ticker}
          onChange={setTicker}
          className="w-full max-w-xs rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm font-mono text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none"
        />
      </div>
      <LegFields label="Entry" side={eSide} setSide={setESide} action={eAction} setAction={setEAction} quantity={eQty} setQuantity={setEQty} price={ePrice} setPrice={setEPrice} tif={eTif} setTif={setETif} />
      <LegFields label="Take Profit" side={tpSide} setSide={setTpSide} action={tpAction} setAction={setTpAction} quantity={tpQty} setQuantity={setTpQty} price={tpPrice} setPrice={setTpPrice} tif={tpTif} setTif={setTpTif} />
      <LegFields label="Stop Loss" side={slSide} setSide={setSlSide} action={slAction} setAction={setSlAction} quantity={slQty} setQuantity={setSlQty} price={slPrice} setPrice={setSlPrice} tif={slTif} setTif={setSlTif} />
      {error && <p className="text-xs text-red">{error}</p>}
      <button type="submit" disabled={submitting} className="rounded-md bg-accent px-4 py-1.5 text-sm font-medium text-fg hover:bg-accent-hover transition-colors disabled:opacity-50">
        {submitting ? "Submitting..." : "Create Bracket"}
      </button>
    </form>
  );
}
