"use client";

import { useState, useMemo } from "react";
import { useMe, useHarmanSessions, useSessionOrders } from "@/lib/hooks";
import { OrderTimeline } from "@/components/OrderTimeline";
import { StateBadge } from "@/components/state-badge";
import type { HarmanSession, Order } from "@/lib/types";

export default function SessionsPage() {
  const { data: me } = useMe();
  const hasAdmin = me?.scopes.includes("harman:admin") || me?.scopes.includes("*");

  if (!me) return <div className="py-10 text-center text-fg-muted">Loading...</div>;
  if (!hasAdmin) return <div className="py-10 text-center text-fg-muted">Requires <code className="font-mono text-accent">harman:admin</code> scope.</div>;

  return <SessionsContent />;
}

function SessionsContent() {
  const { data: sessions, error } = useHarmanSessions();
  const [selected, setSelected] = useState<{ id: number; instance: string } | null>(null);
  const [search, setSearch] = useState("");

  const filtered = useMemo(() => {
    if (!sessions) return undefined;
    if (!search) return sessions;
    const q = search.toLowerCase();
    return sessions.filter(
      (s) =>
        s.exchange.toLowerCase().includes(q) ||
        s.environment.toLowerCase().includes(q) ||
        (s.instance?.toLowerCase().includes(q) ?? false) ||
        (s.display_name?.toLowerCase().includes(q) ?? false) ||
        s.api_key_prefix.toLowerCase().includes(q)
    );
  }, [sessions, search]);

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-bold">Sessions</h1>

      {error && <p className="text-sm text-red">Error loading sessions: {error.message}</p>}

      <div className="space-y-4">
        <div className="flex items-center gap-3">
          <input
            type="text"
            placeholder="Search sessions..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="rounded-md border border-border bg-bg-surface px-3 py-1 text-sm text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none w-48"
          />
          {sessions && <span className="text-xs text-fg-muted">{sessions.length} session{sessions.length !== 1 ? "s" : ""}</span>}
        </div>

        <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-xs text-fg-muted border-b border-border">
                  <th className="px-4 py-2">ID</th>
                  <th className="px-4 py-2">Instance</th>
                  <th className="px-4 py-2">Exchange</th>
                  <th className="px-4 py-2">Env</th>
                  <th className="px-4 py-2">Name</th>
                  <th className="px-4 py-2 text-right">Open / Max</th>
                  <th className="px-4 py-2 text-right">Orders</th>
                  <th className="px-4 py-2 text-right">Fills</th>
                  <th className="px-4 py-2">Status</th>
                  <th className="px-4 py-2">Last Activity</th>
                </tr>
              </thead>
              <tbody>
                {filtered && filtered.length > 0 ? filtered.map((s) => (
                  <SessionRow key={`${s.instance}-${s.id}`} session={s}
                    isSelected={selected?.id === s.id && selected?.instance === s.instance}
                    onToggle={() => setSelected(selected?.id === s.id && selected?.instance === s.instance ? null : { id: s.id, instance: s.instance })}
                  />
                )) : (
                  <tr><td colSpan={10} className="px-4 py-8 text-center text-fg-subtle text-sm">{filtered ? "No sessions" : "Loading..."}</td></tr>
                )}
              </tbody>
            </table>
          </div>
        </div>

        {selected && <SessionOrdersPanel sessionId={selected.id} instance={selected.instance} />}
      </div>
    </div>
  );
}

function SessionRow({ session: s, isSelected, onToggle }: { session: HarmanSession; isSelected: boolean; onToggle: () => void }) {
  return (
    <tr className={`border-b border-border-subtle cursor-pointer transition-colors ${isSelected ? "bg-bg-surface" : "hover:bg-bg-surface-hover"}`} onClick={onToggle}>
      <td className="px-4 py-2 font-mono text-fg-muted">{s.id}</td>
      <td className="px-4 py-2 font-mono text-xs text-fg-muted">{s.instance}</td>
      <td className="px-4 py-2 capitalize">{s.exchange}</td>
      <td className="px-4 py-2">{s.environment}</td>
      <td className="px-4 py-2">{s.display_name || s.api_key_prefix + "..."}</td>
      <td className="px-4 py-2 text-right font-mono">
        <span className={s.open_notional > s.max_notional * 0.8 ? "text-red" : s.open_notional > s.max_notional * 0.5 ? "text-yellow" : "text-green"}>
          ${Number(s.open_notional).toFixed(2)}
        </span>
        <span className="text-fg-muted"> / ${Number(s.max_notional).toFixed(2)}</span>
      </td>
      <td className="px-4 py-2 text-right font-mono">{s.open_order_count}</td>
      <td className="px-4 py-2 text-right font-mono">{s.total_fills}</td>
      <td className="px-4 py-2">
        {s.suspended
          ? <span className="text-xs bg-red/15 text-red px-2 py-0.5 rounded">suspended</span>
          : <span className="text-xs bg-green/15 text-green px-2 py-0.5 rounded">active</span>}
      </td>
      <td className="px-4 py-2 text-xs text-fg-muted font-mono">
        {s.last_activity ? new Date(s.last_activity).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) : "—"}
      </td>
    </tr>
  );
}

function SessionOrdersPanel({ sessionId, instance }: { sessionId: number; instance?: string }) {
  const { data: orders, error } = useSessionOrders(sessionId, instance);
  const [expandedOrder, setExpandedOrder] = useState<number | null>(null);

  if (error) return <p className="text-xs text-red">Error loading orders: {error.message}</p>;
  if (!orders) return <p className="text-xs text-fg-muted">Loading orders...</p>;

  return (
    <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
      <div className="px-4 py-2 border-b border-border">
        <span className="text-xs font-medium text-fg">Session #{sessionId} Orders</span>
        <span className="text-xs text-fg-muted ml-2">{orders.length} total</span>
      </div>
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-xs text-fg-muted border-b border-border">
              <th className="px-4 py-2">ID</th>
              <th className="px-4 py-2">Ticker</th>
              <th className="px-4 py-2">Side</th>
              <th className="px-4 py-2">Action</th>
              <th className="px-4 py-2 text-right">Qty</th>
              <th className="px-4 py-2 text-right">Price</th>
              <th className="px-4 py-2">State</th>
              <th className="px-4 py-2">Created</th>
            </tr>
          </thead>
          <tbody>
            {orders.length > 0 ? orders.map((o) => (
              <OrderRow key={o.id} order={o} instance={instance} isExpanded={expandedOrder === o.id}
                onToggle={() => setExpandedOrder(expandedOrder === o.id ? null : o.id)} />
            )) : (
              <tr><td colSpan={8} className="px-4 py-6 text-center text-fg-subtle text-sm">No orders for this session</td></tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function OrderRow({ order: o, instance, isExpanded, onToggle }: { order: Order; instance?: string; isExpanded: boolean; onToggle: () => void }) {
  return (
    <>
      <tr className={`border-b border-border-subtle cursor-pointer transition-colors ${isExpanded ? "bg-bg-surface" : "hover:bg-bg-surface-hover"}`} onClick={onToggle}>
        <td className="px-4 py-2 font-mono text-fg-muted">{o.id}</td>
        <td className="px-4 py-2 font-mono text-xs">{o.ticker}</td>
        <td className="px-4 py-2 uppercase text-xs">{o.side}</td>
        <td className="px-4 py-2 uppercase text-xs">{o.action}</td>
        <td className="px-4 py-2 font-mono text-right">{Number(o.filled_quantity).toFixed(0)}/{Number(o.quantity).toFixed(0)}</td>
        <td className="px-4 py-2 font-mono text-right">${Number(o.price_dollars).toFixed(2)}</td>
        <td className="px-4 py-2">
          <StateBadge state={o.state} />
          {o.cancel_reason && <span className="ml-1.5 text-xs text-fg-muted">({o.cancel_reason.replace(/_/g, " ")})</span>}
        </td>
        <td className="px-4 py-2 text-xs text-fg-muted font-mono">
          {new Date(o.created_at).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}
        </td>
      </tr>
      {isExpanded && (
        <tr><td colSpan={8} className="px-4 py-3 bg-bg"><OrderTimeline orderId={o.id} instance={instance} /></td></tr>
      )}
    </>
  );
}
