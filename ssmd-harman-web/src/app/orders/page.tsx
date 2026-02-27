"use client";

import { useState } from "react";
import { useOrders } from "@/lib/hooks";
import { pump } from "@/lib/api";
import { StateBadge } from "@/components/state-badge";
import { OrderActions } from "@/components/order-actions";
import { CreateOrderForm } from "@/components/create-order-form";

const stateFilters = [
  { value: "", label: "All" },
  { value: "open", label: "Open" },
  { value: "resting", label: "Resting" },
  { value: "terminal", label: "Terminal" },
  { value: "staged", label: "Staged" },
  { value: "today", label: "Today" },
];

export default function OrdersPage() {
  const [filter, setFilter] = useState("");
  const { data: orders, error } = useOrders(filter || undefined);
  const [pumpMsg, setPumpMsg] = useState("");

  async function handlePump() {
    setPumpMsg("");
    try {
      await pump();
      setPumpMsg("Pump completed");
    } catch (err) {
      setPumpMsg(`Pump failed: ${err instanceof Error ? err.message : "unknown"}`);
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold">Orders</h1>
        <div className="flex items-center gap-3">
          <select
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            className="rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm text-fg focus:border-accent focus:outline-none"
          >
            {stateFilters.map((f) => (
              <option key={f.value} value={f.value}>{f.label}</option>
            ))}
          </select>
          <button onClick={handlePump} className="rounded-md bg-accent px-4 py-1.5 text-sm font-medium text-fg hover:bg-accent-hover transition-colors">
            Pump
          </button>
        </div>
      </div>
      {pumpMsg && <p className="text-xs text-fg-muted">{pumpMsg}</p>}

      <CreateOrderForm />

      {error && <p className="text-sm text-red">Error loading orders: {error.message}</p>}

      <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-fg-muted border-b border-border">
                <th className="px-4 py-2">ID</th>
                <th className="px-4 py-2">Ticker</th>
                <th className="px-4 py-2">Side</th>
                <th className="px-4 py-2">Action</th>
                <th className="px-4 py-2 text-right">Qty</th>
                <th className="px-4 py-2 text-right">Filled</th>
                <th className="px-4 py-2 text-right">Price</th>
                <th className="px-4 py-2">TIF</th>
                <th className="px-4 py-2">State</th>
                <th className="px-4 py-2">Leg</th>
                <th className="px-4 py-2">Created</th>
                <th className="px-4 py-2">Actions</th>
              </tr>
            </thead>
            <tbody>
              {orders && orders.length > 0 ? (
                orders.map((o) => (
                  <tr key={o.id} className="border-b border-border-subtle hover:bg-bg-surface-hover">
                    <td className="px-4 py-2 font-mono text-fg-muted">{o.id}</td>
                    <td className="px-4 py-2 font-mono">{o.ticker}</td>
                    <td className="px-4 py-2 uppercase">{o.side}</td>
                    <td className="px-4 py-2 uppercase">{o.action}</td>
                    <td className="px-4 py-2 font-mono text-right">{o.quantity}</td>
                    <td className="px-4 py-2 font-mono text-right">{o.filled_quantity}</td>
                    <td className="px-4 py-2 font-mono text-right">${o.price_dollars}</td>
                    <td className="px-4 py-2 uppercase text-xs">{o.time_in_force}</td>
                    <td className="px-4 py-2"><StateBadge state={o.state} /></td>
                    <td className="px-4 py-2 text-xs text-fg-muted">{o.leg_role || "-"}</td>
                    <td className="px-4 py-2 text-xs text-fg-muted font-mono">{new Date(o.created_at).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })}</td>
                    <td className="px-4 py-2"><OrderActions order={o} /></td>
                  </tr>
                ))
              ) : (
                <tr>
                  <td colSpan={12} className="px-4 py-8 text-center text-fg-subtle text-sm">
                    {orders ? "No orders" : "Loading..."}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
