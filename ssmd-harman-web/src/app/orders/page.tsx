"use client";

import { useState, useMemo } from "react";
import { useOrders } from "@/lib/hooks";
import { pump } from "@/lib/api";
import { StateBadge } from "@/components/state-badge";
import { OrderActions } from "@/components/order-actions";
import { CreateOrderForm } from "@/components/create-order-form";
import type { Order } from "@/lib/types";

type SortKey = "id" | "ticker" | "quantity" | "price_dollars" | "state" | "created_at";
type SortDir = "asc" | "desc";

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
  const [sortKey, setSortKey] = useState<SortKey>("created_at");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  function handleSort(key: SortKey) {
    if (sortKey === key) {
      setSortDir(sortDir === "asc" ? "desc" : "asc");
    } else {
      setSortKey(key);
      setSortDir(key === "created_at" || key === "id" ? "desc" : "asc");
    }
  }

  const sortedOrders = useMemo(() => {
    if (!orders) return undefined;
    return [...orders].sort((a, b) => {
      let cmp = 0;
      switch (sortKey) {
        case "id": cmp = a.id - b.id; break;
        case "ticker": cmp = a.ticker.localeCompare(b.ticker); break;
        case "quantity": cmp = Number(a.quantity) - Number(b.quantity); break;
        case "price_dollars": cmp = Number(a.price_dollars) - Number(b.price_dollars); break;
        case "state": cmp = a.state.localeCompare(b.state); break;
        case "created_at": cmp = a.created_at.localeCompare(b.created_at); break;
      }
      return sortDir === "asc" ? cmp : -cmp;
    });
  }, [orders, sortKey, sortDir]);

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
                <SortTh k="id" current={sortKey} dir={sortDir} onClick={handleSort}>ID</SortTh>
                <SortTh k="ticker" current={sortKey} dir={sortDir} onClick={handleSort}>Ticker</SortTh>
                <th className="px-4 py-2">Side</th>
                <th className="px-4 py-2">Action</th>
                <SortTh k="quantity" current={sortKey} dir={sortDir} onClick={handleSort} align="right">Qty</SortTh>
                <th className="px-4 py-2 text-right">Filled</th>
                <SortTh k="price_dollars" current={sortKey} dir={sortDir} onClick={handleSort} align="right">Price</SortTh>
                <th className="px-4 py-2">TIF</th>
                <SortTh k="state" current={sortKey} dir={sortDir} onClick={handleSort}>State</SortTh>
                <th className="px-4 py-2">Leg</th>
                <SortTh k="created_at" current={sortKey} dir={sortDir} onClick={handleSort}>Created</SortTh>
                <th className="px-4 py-2">Actions</th>
              </tr>
            </thead>
            <tbody>
              {sortedOrders && sortedOrders.length > 0 ? (
                sortedOrders.map((o) => (
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
                    {sortedOrders ? "No orders" : "Loading..."}
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

function SortTh({ k, current, dir, onClick, align, children }: {
  k: SortKey; current: SortKey; dir: SortDir;
  onClick: (k: SortKey) => void; align?: "right"; children: React.ReactNode;
}) {
  const active = current === k;
  const arrow = active ? (dir === "asc" ? " \u25B2" : " \u25BC") : "";
  return (
    <th
      className={`px-4 py-2 cursor-pointer select-none hover:text-fg transition-colors ${align === "right" ? "text-right" : ""} ${active ? "text-fg" : ""}`}
      onClick={() => onClick(k)}
    >
      {children}{arrow}
    </th>
  );
}
