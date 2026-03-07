"use client";

import { useState, useEffect } from "react";
import { v4 as uuidv4 } from "uuid";
import { createBracket } from "@/lib/api";
import type { Side, Action, TimeInForce } from "@/lib/types";
import { useSWRConfig } from "swr";
import { matchInstanceKey } from "@/lib/hooks";

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
      <div className="grid grid-cols-2 gap-2">
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

interface Props {
  ticker: string;
  yesBid: number | null;
  yesAsk: number | null;
  last: number | null;
  instanceId?: string;
  onSuccess?: () => void;
  initialSide?: Side;
  initialAction?: Action;
  initialPrice?: string;
}

export function CreateBracketFormControlled({
  ticker,
  yesBid,
  yesAsk,
  last,
  instanceId,
  onSuccess,
  initialSide,
  initialAction,
  initialPrice,
}: Props) {
  const { mutate } = useSWRConfig();
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);

  // Entry leg
  const [eSide, setESide] = useState<Side>(initialSide ?? "yes");
  const [eAction, setEAction] = useState<Action>(initialAction ?? "buy");
  const [eQty, setEQty] = useState("");
  const [ePrice, setEPrice] = useState(initialPrice ?? "");
  const [eTif, setETif] = useState<TimeInForce>("gtc");

  // Take profit leg — same side as entry, opposite action
  const [tpSide, setTpSide] = useState<Side>(initialSide ?? "yes");
  const [tpAction, setTpAction] = useState<Action>(
    initialAction === "sell" ? "buy" : "sell"
  );
  const [tpQty, setTpQty] = useState("");
  const [tpPrice, setTpPrice] = useState("");
  const [tpTif, setTpTif] = useState<TimeInForce>("gtc");

  // Stop loss leg — opposite side, sell action
  const [slSide, setSlSide] = useState<Side>(
    (initialSide ?? "yes") === "yes" ? "no" : "yes"
  );
  const [slAction, setSlAction] = useState<Action>("sell");
  const [slQty, setSlQty] = useState("");
  const [slPrice, setSlPrice] = useState("");
  const [slTrigger, setSlTrigger] = useState("");
  const [slTif, setSlTif] = useState<TimeInForce>("gtc");

  // Sync entry leg when initial values change
  useEffect(() => {
    if (initialSide) {
      setESide(initialSide);
      setTpSide(initialSide);
      setSlSide(initialSide === "yes" ? "no" : "yes");
    }
    if (initialAction) {
      setEAction(initialAction);
      setTpAction(initialAction === "sell" ? "buy" : "sell");
    }
    if (initialPrice) setEPrice(initialPrice);
  }, [initialSide, initialAction, initialPrice]);

  const fmtPrice = (v: number | null) => (v != null ? v.toFixed(2) : "\u2014");

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    setSubmitting(true);
    try {
      await createBracket(
        {
          entry: {
            client_order_id: uuidv4(),
            ticker,
            side: eSide,
            action: eAction,
            quantity: eQty,
            price_dollars: ePrice,
            time_in_force: eTif,
          },
          take_profit: {
            client_order_id: uuidv4(),
            ticker,
            side: tpSide,
            action: tpAction,
            quantity: tpQty,
            price_dollars: tpPrice,
            time_in_force: tpTif,
          },
          stop_loss: {
            client_order_id: uuidv4(),
            ticker,
            side: slSide,
            action: slAction,
            quantity: slQty,
            price_dollars: slPrice,
            time_in_force: slTif,
            ...(slTrigger ? { trigger_price: slTrigger, order_type: "market" as const } : {}),
          },
        },
        instanceId
      );
      mutate(matchInstanceKey("groups"));
      onSuccess?.();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create bracket");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      {/* Bid/Ask context */}
      <div className="flex gap-4 text-xs text-fg-muted">
        <span>
          Bid: <span className="font-mono text-fg">${fmtPrice(yesBid)}</span>
        </span>
        <span>
          Ask: <span className="font-mono text-fg">${fmtPrice(yesAsk)}</span>
        </span>
        <span>
          Last: <span className="font-mono text-fg">${fmtPrice(last)}</span>
        </span>
      </div>

      <LegFields
        label="Entry"
        side={eSide}
        setSide={setESide}
        action={eAction}
        setAction={setEAction}
        quantity={eQty}
        setQuantity={setEQty}
        price={ePrice}
        setPrice={setEPrice}
        tif={eTif}
        setTif={setETif}
      />
      <LegFields
        label="Take Profit"
        side={tpSide}
        setSide={setTpSide}
        action={tpAction}
        setAction={setTpAction}
        quantity={tpQty}
        setQuantity={setTpQty}
        price={tpPrice}
        setPrice={setTpPrice}
        tif={tpTif}
        setTif={setTpTif}
      />
      <LegFields
        label="Stop Loss"
        side={slSide}
        setSide={setSlSide}
        action={slAction}
        setAction={setSlAction}
        quantity={slQty}
        setQuantity={setSlQty}
        price={slPrice}
        setPrice={setSlPrice}
        tif={slTif}
        setTif={setSlTif}
      />
      <div className="space-y-1">
        <span className="text-xs font-medium text-fg-muted uppercase">SL Trigger Price</span>
        <input
          type="text"
          value={slTrigger}
          onChange={(e) => setSlTrigger(e.target.value)}
          placeholder="Trigger (e.g. 0.40)"
          className="w-full rounded border border-border bg-bg-surface px-2 py-1 text-xs font-mono text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none"
        />
        <p className="text-[10px] text-fg-subtle">
          When set, SL stays staged until market price hits trigger, then submits as IOC market order.
        </p>
      </div>

      {error && (
        <div className="rounded-md border border-red bg-red/10 px-3 py-2">
          <p className="text-sm font-medium text-red">Order failed</p>
          <p className="text-xs text-red/80 mt-0.5">{error}</p>
        </div>
      )}
      <button
        type="submit"
        disabled={submitting}
        className="w-full rounded-md bg-accent px-4 py-1.5 text-sm font-medium text-fg hover:bg-accent-hover transition-colors disabled:opacity-50"
      >
        {submitting ? "Submitting..." : "Create Bracket"}
      </button>
    </form>
  );
}
