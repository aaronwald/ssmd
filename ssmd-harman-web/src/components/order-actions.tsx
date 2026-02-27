"use client";

import { useState } from "react";
import { cancelOrder, amendOrder, decreaseOrder } from "@/lib/api";
import type { Order, OrderState } from "@/lib/types";
import { useSWRConfig } from "swr";

const cancellableStates: OrderState[] = [
  "pending",
  "submitted",
  "acknowledged",
  "partially_filled",
  "staged",
];

const amendableStates: OrderState[] = [
  "acknowledged",
  "partially_filled",
];

export function OrderActions({ order }: { order: Order }) {
  const { mutate } = useSWRConfig();
  const [mode, setMode] = useState<"idle" | "amend" | "decrease">("idle");
  const [newPrice, setNewPrice] = useState(order.price_dollars);
  const [newQty, setNewQty] = useState(order.quantity);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const canCancel = cancellableStates.includes(order.state);
  const canAmend = amendableStates.includes(order.state);

  async function handleCancel() {
    setLoading(true);
    setError("");
    try {
      await cancelOrder(order.id);
      mutate((key: string) => typeof key === "string" && key.startsWith("orders"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Cancel failed");
    } finally {
      setLoading(false);
    }
  }

  async function handleAmend() {
    setLoading(true);
    setError("");
    try {
      await amendOrder(order.id, { price_dollars: newPrice, quantity: newQty });
      setMode("idle");
      mutate((key: string) => typeof key === "string" && key.startsWith("orders"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Amend failed");
    } finally {
      setLoading(false);
    }
  }

  async function handleDecrease() {
    setLoading(true);
    setError("");
    try {
      await decreaseOrder(order.id, { quantity: newQty });
      setMode("idle");
      mutate((key: string) => typeof key === "string" && key.startsWith("orders"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Decrease failed");
    } finally {
      setLoading(false);
    }
  }

  if (mode === "amend") {
    return (
      <div className="flex items-center gap-2">
        <input
          type="text"
          value={newPrice}
          onChange={(e) => setNewPrice(e.target.value)}
          className="w-20 rounded border border-border bg-bg-surface px-2 py-1 text-xs font-mono text-fg focus:border-accent focus:outline-none"
          placeholder="Price"
        />
        <input
          type="text"
          value={newQty}
          onChange={(e) => setNewQty(e.target.value)}
          className="w-16 rounded border border-border bg-bg-surface px-2 py-1 text-xs font-mono text-fg focus:border-accent focus:outline-none"
          placeholder="Qty"
        />
        <button onClick={handleAmend} disabled={loading} className="text-xs text-green hover:text-green/80">OK</button>
        <button onClick={() => setMode("idle")} className="text-xs text-fg-muted hover:text-fg">X</button>
        {error && <span className="text-xs text-red">{error}</span>}
      </div>
    );
  }

  if (mode === "decrease") {
    return (
      <div className="flex items-center gap-2">
        <input
          type="text"
          value={newQty}
          onChange={(e) => setNewQty(e.target.value)}
          className="w-16 rounded border border-border bg-bg-surface px-2 py-1 text-xs font-mono text-fg focus:border-accent focus:outline-none"
          placeholder="New qty"
        />
        <button onClick={handleDecrease} disabled={loading} className="text-xs text-green hover:text-green/80">OK</button>
        <button onClick={() => setMode("idle")} className="text-xs text-fg-muted hover:text-fg">X</button>
        {error && <span className="text-xs text-red">{error}</span>}
      </div>
    );
  }

  return (
    <div className="flex items-center gap-2">
      {canCancel && (
        <button onClick={handleCancel} disabled={loading} className="text-xs text-red hover:text-red/80">
          Cancel
        </button>
      )}
      {canAmend && (
        <button onClick={() => setMode("amend")} className="text-xs text-accent hover:text-accent-hover">
          Amend
        </button>
      )}
      {canAmend && (
        <button onClick={() => setMode("decrease")} className="text-xs text-orange hover:text-orange/80">
          Decrease
        </button>
      )}
      {error && <span className="text-xs text-red">{error}</span>}
    </div>
  );
}
