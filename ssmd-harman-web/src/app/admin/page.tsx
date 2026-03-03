"use client";

import { useState, useMemo } from "react";
import {
  useMe,
  useAdminUsers,
  useHarmanSessions,
  useSessionOrders,
  useExchangeAudit,
} from "@/lib/hooks";
import { ArchitectureDiagram } from "@/components/ArchitectureDiagram";
import { OrderTimeline } from "@/components/OrderTimeline";
import { StateBadge } from "@/components/state-badge";
import type {
  AdminKey,
  AdminSession,
  HarmanSession,
  Order,
  ExchangeAuditEntry,
} from "@/lib/types";

type AdminTab = "sessions" | "keys" | "exchange-audit";

export default function AdminPage() {
  const { data: me } = useMe();
  const hasAdmin =
    me?.scopes.includes("harman:admin") || me?.scopes.includes("*");

  if (!me) {
    return <div className="py-10 text-center text-fg-muted">Loading...</div>;
  }

  if (!hasAdmin) {
    return (
      <div className="py-10 text-center">
        <h1 className="text-xl font-bold text-fg mb-2">Admin</h1>
        <p className="text-sm text-fg-muted">
          You do not have the{" "}
          <code className="font-mono text-accent">harman:admin</code> scope.
        </p>
      </div>
    );
  }

  return <AdminContent />;
}

function AdminContent() {
  const [tab, setTab] = useState<AdminTab>("sessions");

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold">Admin</h1>
        <div className="flex gap-1">
          {(["sessions", "keys", "exchange-audit"] as const).map((t) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={`rounded-md px-3 py-1 text-sm transition-colors ${
                tab === t
                  ? "bg-accent text-fg"
                  : "text-fg-muted hover:text-fg"
              }`}
            >
              {t === "sessions"
                ? "Sessions"
                : t === "keys"
                ? "API Keys"
                : "Exchange Audit"}
            </button>
          ))}
        </div>
      </div>

      {/* Architecture diagram — always visible */}
      <ArchitectureDiagram />

      {tab === "sessions" && <SessionsSection />}
      {tab === "keys" && <KeysSection />}
      {tab === "exchange-audit" && <ExchangeAuditSection />}
    </div>
  );
}

/* ─── Sessions Tab ─── */

function SessionsSection() {
  const { data: sessions, error } = useHarmanSessions();
  const [selectedSession, setSelectedSession] = useState<number | null>(null);
  const [search, setSearch] = useState("");

  const filtered = useMemo(() => {
    if (!sessions) return undefined;
    if (!search) return sessions;
    const q = search.toLowerCase();
    return sessions.filter(
      (s) =>
        s.exchange.toLowerCase().includes(q) ||
        s.environment.toLowerCase().includes(q) ||
        (s.display_name?.toLowerCase().includes(q) ?? false) ||
        s.api_key_prefix.toLowerCase().includes(q)
    );
  }, [sessions, search]);

  if (error) {
    return (
      <p className="text-sm text-red">
        Error loading sessions: {error.message}
      </p>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <h2 className="text-sm font-semibold text-fg">Trading Sessions</h2>
        <input
          type="text"
          placeholder="Search sessions..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="rounded-md border border-border bg-bg-surface px-3 py-1 text-sm text-fg placeholder:text-fg-subtle focus:border-accent focus:outline-none w-48"
        />
        {sessions && (
          <span className="text-xs text-fg-muted">
            {sessions.length} session{sessions.length !== 1 ? "s" : ""}
          </span>
        )}
      </div>

      <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-fg-muted border-b border-border">
                <th className="px-4 py-2">ID</th>
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
              {filtered && filtered.length > 0 ? (
                filtered.map((s) => (
                  <SessionRow
                    key={s.id}
                    session={s}
                    isSelected={selectedSession === s.id}
                    onToggle={() =>
                      setSelectedSession(
                        selectedSession === s.id ? null : s.id
                      )
                    }
                  />
                ))
              ) : (
                <tr>
                  <td
                    colSpan={9}
                    className="px-4 py-8 text-center text-fg-subtle text-sm"
                  >
                    {filtered ? "No sessions" : "Loading..."}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* Expanded session orders */}
      {selectedSession && <SessionOrdersPanel sessionId={selectedSession} />}
    </div>
  );
}

function SessionRow({
  session: s,
  isSelected,
  onToggle,
}: {
  session: HarmanSession;
  isSelected: boolean;
  onToggle: () => void;
}) {
  return (
    <tr
      className={`border-b border-border-subtle cursor-pointer transition-colors ${
        isSelected ? "bg-bg-surface" : "hover:bg-bg-surface-hover"
      }`}
      onClick={onToggle}
    >
      <td className="px-4 py-2 font-mono text-fg-muted">{s.id}</td>
      <td className="px-4 py-2 capitalize">{s.exchange}</td>
      <td className="px-4 py-2">{s.environment}</td>
      <td className="px-4 py-2">{s.display_name || s.api_key_prefix + "..."}</td>
      <td className="px-4 py-2 text-right font-mono">
        <span
          className={
            s.open_notional > s.max_notional * 0.8
              ? "text-red"
              : s.open_notional > s.max_notional * 0.5
              ? "text-yellow"
              : "text-green"
          }
        >
          ${Number(s.open_notional).toFixed(2)}
        </span>
        <span className="text-fg-muted"> / ${Number(s.max_notional).toFixed(2)}</span>
      </td>
      <td className="px-4 py-2 text-right font-mono">{s.open_order_count}</td>
      <td className="px-4 py-2 text-right font-mono">{s.total_fills}</td>
      <td className="px-4 py-2">
        {s.suspended ? (
          <span className="text-xs bg-red/15 text-red px-2 py-0.5 rounded">
            suspended
          </span>
        ) : (
          <span className="text-xs bg-green/15 text-green px-2 py-0.5 rounded">
            active
          </span>
        )}
      </td>
      <td className="px-4 py-2 text-xs text-fg-muted font-mono">
        {s.last_activity
          ? new Date(s.last_activity).toLocaleString([], {
              month: "short",
              day: "numeric",
              hour: "2-digit",
              minute: "2-digit",
            })
          : "—"}
      </td>
    </tr>
  );
}

function SessionOrdersPanel({ sessionId }: { sessionId: number }) {
  const { data: orders, error } = useSessionOrders(sessionId);
  const [expandedOrder, setExpandedOrder] = useState<number | null>(null);

  if (error) {
    return (
      <p className="text-xs text-red">
        Error loading orders: {error.message}
      </p>
    );
  }

  if (!orders) {
    return <p className="text-xs text-fg-muted">Loading orders...</p>;
  }

  return (
    <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
      <div className="px-4 py-2 border-b border-border">
        <span className="text-xs font-medium text-fg">
          Session #{sessionId} Orders
        </span>
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
            {orders.length > 0 ? (
              orders.map((o) => (
                <OrderRow
                  key={o.id}
                  order={o}
                  isExpanded={expandedOrder === o.id}
                  onToggle={() =>
                    setExpandedOrder(expandedOrder === o.id ? null : o.id)
                  }
                />
              ))
            ) : (
              <tr>
                <td
                  colSpan={8}
                  className="px-4 py-6 text-center text-fg-subtle text-sm"
                >
                  No orders for this session
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function OrderRow({
  order: o,
  isExpanded,
  onToggle,
}: {
  order: Order;
  isExpanded: boolean;
  onToggle: () => void;
}) {
  return (
    <>
      <tr
        className={`border-b border-border-subtle cursor-pointer transition-colors ${
          isExpanded ? "bg-bg-surface" : "hover:bg-bg-surface-hover"
        }`}
        onClick={onToggle}
      >
        <td className="px-4 py-2 font-mono text-fg-muted">{o.id}</td>
        <td className="px-4 py-2 font-mono text-xs">{o.ticker}</td>
        <td className="px-4 py-2 uppercase text-xs">{o.side}</td>
        <td className="px-4 py-2 uppercase text-xs">{o.action}</td>
        <td className="px-4 py-2 font-mono text-right">
          {o.filled_quantity}/{o.quantity}
        </td>
        <td className="px-4 py-2 font-mono text-right">${o.price_dollars}</td>
        <td className="px-4 py-2">
          <StateBadge state={o.state} />
        </td>
        <td className="px-4 py-2 text-xs text-fg-muted font-mono">
          {new Date(o.created_at).toLocaleString([], {
            month: "short",
            day: "numeric",
            hour: "2-digit",
            minute: "2-digit",
          })}
        </td>
      </tr>
      {isExpanded && (
        <tr>
          <td colSpan={8} className="px-4 py-3 bg-bg">
            <OrderTimeline orderId={o.id} />
          </td>
        </tr>
      )}
    </>
  );
}

/* ─── Exchange Audit Tab ─── */

function ExchangeAuditSection() {
  const { data: sessions } = useHarmanSessions();
  const [selectedSession, setSelectedSession] = useState<number | null>(null);

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <h2 className="text-sm font-semibold text-fg">Exchange Audit Log</h2>
        <select
          value={selectedSession ?? ""}
          onChange={(e) =>
            setSelectedSession(e.target.value ? Number(e.target.value) : null)
          }
          className="rounded-md border border-border bg-bg-surface px-3 py-1 text-sm text-fg focus:border-accent focus:outline-none"
        >
          <option value="">Select session...</option>
          {sessions?.map((s) => (
            <option key={s.id} value={s.id}>
              #{s.id} — {s.exchange} ({s.environment})
              {s.display_name ? ` — ${s.display_name}` : ""}
            </option>
          ))}
        </select>
      </div>

      {selectedSession && <AuditLogTable sessionId={selectedSession} />}

      {!selectedSession && (
        <p className="text-sm text-fg-muted">
          Select a session to view exchange audit entries.
        </p>
      )}
    </div>
  );
}

function AuditLogTable({ sessionId }: { sessionId: number }) {
  const { data: audit, error } = useExchangeAudit(sessionId);
  const [categoryFilter, setCategoryFilter] = useState("");
  const [outcomeFilter, setOutcomeFilter] = useState("");

  const filtered = useMemo(() => {
    if (!audit) return undefined;
    return audit.filter((a) => {
      if (categoryFilter && a.category !== categoryFilter) return false;
      if (outcomeFilter && a.outcome !== outcomeFilter) return false;
      return true;
    });
  }, [audit, categoryFilter, outcomeFilter]);

  const categories = useMemo(() => {
    if (!audit) return [];
    return [...new Set(audit.map((a) => a.category))].sort();
  }, [audit]);

  const outcomes = useMemo(() => {
    if (!audit) return [];
    return [...new Set(audit.map((a) => a.outcome))].sort();
  }, [audit]);

  if (error) {
    return (
      <p className="text-sm text-red">
        Error loading audit: {error.message}
      </p>
    );
  }

  return (
    <div className="space-y-3">
      <div className="flex gap-2">
        <select
          value={categoryFilter}
          onChange={(e) => setCategoryFilter(e.target.value)}
          className="rounded-md border border-border bg-bg-surface px-2 py-1 text-xs text-fg focus:border-accent focus:outline-none"
        >
          <option value="">All categories</option>
          {categories.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
        <select
          value={outcomeFilter}
          onChange={(e) => setOutcomeFilter(e.target.value)}
          className="rounded-md border border-border bg-bg-surface px-2 py-1 text-xs text-fg focus:border-accent focus:outline-none"
        >
          <option value="">All outcomes</option>
          {outcomes.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
        {audit && (
          <span className="text-xs text-fg-muted self-center">
            {filtered?.length} / {audit.length} entries
          </span>
        )}
      </div>

      <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-fg-muted border-b border-border">
                <th className="px-3 py-2">Time</th>
                <th className="px-3 py-2">Category</th>
                <th className="px-3 py-2">Action</th>
                <th className="px-3 py-2">Order</th>
                <th className="px-3 py-2">Endpoint</th>
                <th className="px-3 py-2 text-right">Status</th>
                <th className="px-3 py-2 text-right">Duration</th>
                <th className="px-3 py-2">Outcome</th>
                <th className="px-3 py-2">Error</th>
              </tr>
            </thead>
            <tbody>
              {filtered && filtered.length > 0 ? (
                filtered.map((a) => (
                  <AuditRow key={a.id} entry={a} />
                ))
              ) : (
                <tr>
                  <td
                    colSpan={9}
                    className="px-4 py-6 text-center text-fg-subtle text-sm"
                  >
                    {filtered ? "No audit entries" : "Loading..."}
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

const categoryColors: Record<string, string> = {
  rest_call: "bg-green/15 text-green",
  ws_event: "bg-blue-light/15 text-blue-light",
  fallback: "bg-orange/15 text-orange",
  reconciliation: "bg-yellow/15 text-yellow",
  recovery: "bg-yellow/15 text-yellow",
  risk: "bg-red/15 text-red",
};

const outcomeColors: Record<string, string> = {
  success: "text-green",
  error: "text-red",
  not_found: "text-orange",
  rate_limited: "text-yellow",
  timeout: "text-yellow",
};

function AuditRow({ entry: a }: { entry: ExchangeAuditEntry }) {
  return (
    <tr className="border-b border-border-subtle hover:bg-bg-surface-hover text-xs">
      <td className="px-3 py-1.5 font-mono text-fg-muted whitespace-nowrap">
        {new Date(a.created_at).toLocaleTimeString([], {
          hour: "2-digit",
          minute: "2-digit",
          second: "2-digit",
        })}
      </td>
      <td className="px-3 py-1.5">
        <span
          className={`inline-block rounded px-1.5 py-0.5 font-mono ${
            categoryColors[a.category] || "bg-fg-subtle/15 text-fg-subtle"
          }`}
        >
          {a.category}
        </span>
      </td>
      <td className="px-3 py-1.5 font-mono text-fg">{a.action}</td>
      <td className="px-3 py-1.5 font-mono text-fg-muted">
        {a.order_id ?? "—"}
      </td>
      <td className="px-3 py-1.5 text-fg-muted truncate max-w-[200px]">
        {a.endpoint ?? "—"}
      </td>
      <td className="px-3 py-1.5 text-right font-mono">
        {a.status_code ? (
          <span
            className={
              a.status_code >= 400 ? "text-red" : "text-green"
            }
          >
            {a.status_code}
          </span>
        ) : (
          "—"
        )}
      </td>
      <td className="px-3 py-1.5 text-right font-mono text-fg-muted">
        {a.duration_ms != null ? `${a.duration_ms}ms` : "—"}
      </td>
      <td className="px-3 py-1.5">
        <span className={outcomeColors[a.outcome] || "text-fg-muted"}>
          {a.outcome}
        </span>
      </td>
      <td className="px-3 py-1.5 text-red truncate max-w-[200px]" title={a.error_msg ?? undefined}>
        {a.error_msg ?? ""}
      </td>
    </tr>
  );
}

/* ─── API Keys Tab (original admin view) ─── */

function KeysSection() {
  const { data, error } = useAdminUsers();
  const [expandedKey, setExpandedKey] = useState<string | null>(null);

  if (error) {
    return (
      <p className="text-sm text-red">
        Error loading admin data: {error.message}
      </p>
    );
  }

  if (!data) {
    return <p className="text-sm text-fg-muted">Loading...</p>;
  }

  const { keys, sessions } = data;

  const byEmail = new Map<string, AdminKey[]>();
  for (const key of keys) {
    const email = key.email || "(no email)";
    const existing = byEmail.get(email) || [];
    existing.push(key);
    byEmail.set(email, existing);
  }

  const sessionsByPrefix = new Map<string, AdminSession[]>();
  for (const s of sessions) {
    const existing = sessionsByPrefix.get(s.key_prefix) || [];
    existing.push(s);
    sessionsByPrefix.set(s.key_prefix, existing);
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <h2 className="text-sm font-semibold text-fg">API Keys & Users</h2>
        <span className="text-xs text-fg-muted">
          {keys.length} keys, {sessions.length} sessions
        </span>
      </div>

      {Array.from(byEmail.entries()).map(([email, userKeys]) => (
        <div
          key={email}
          className="bg-bg-raised border border-border rounded-lg overflow-hidden"
        >
          <div className="px-4 py-3 border-b border-border">
            <span className="text-sm font-medium text-fg">{email}</span>
            <span className="text-xs text-fg-muted ml-2">
              {userKeys.length} key{userKeys.length !== 1 ? "s" : ""}
            </span>
          </div>

          <div className="divide-y divide-border-subtle">
            {userKeys.map((key) => {
              const isExpanded = expandedKey === key.prefix;
              const keySessions = sessionsByPrefix.get(key.prefix) || [];

              return (
                <div key={key.prefix}>
                  <button
                    onClick={() =>
                      setExpandedKey(isExpanded ? null : key.prefix)
                    }
                    className="w-full text-left px-4 py-2 hover:bg-bg-surface-hover transition-colors"
                  >
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <span className="font-mono text-xs text-fg">
                          {key.prefix}...
                        </span>
                        {key.name && (
                          <span className="text-xs text-fg-muted">
                            {key.name}
                          </span>
                        )}
                      </div>
                      <div className="flex items-center gap-2">
                        {keySessions.some((s) => s.suspended) && (
                          <span className="text-xs bg-red/15 text-red px-2 py-0.5 rounded">
                            suspended
                          </span>
                        )}
                        <span className="text-xs text-fg-muted">
                          {isExpanded ? "\u25B2" : "\u25BC"}
                        </span>
                      </div>
                    </div>
                    <div className="flex gap-2 mt-1 flex-wrap">
                      {key.scopes.map((scope) => (
                        <span
                          key={scope}
                          className="text-xs bg-accent/10 text-accent px-1.5 py-0.5 rounded font-mono"
                        >
                          {scope}
                        </span>
                      ))}
                    </div>
                  </button>

                  {isExpanded && (
                    <div className="px-4 py-3 bg-bg text-xs space-y-3">
                      <div className="grid grid-cols-2 gap-2">
                        <div>
                          <span className="text-fg-muted">Tier: </span>
                          <span className="font-mono text-fg">
                            {key.rate_limit_tier || "default"}
                          </span>
                        </div>
                        <div>
                          <span className="text-fg-muted">Feeds: </span>
                          <span className="font-mono text-fg">
                            {key.feeds?.join(", ") || "all"}
                          </span>
                        </div>
                        {key.expires_at && (
                          <div>
                            <span className="text-fg-muted">Expires: </span>
                            <span className="font-mono text-fg">
                              {new Date(key.expires_at).toLocaleDateString()}
                            </span>
                          </div>
                        )}
                        {key.last_used_at && (
                          <div>
                            <span className="text-fg-muted">Last used: </span>
                            <span className="font-mono text-fg">
                              {new Date(key.last_used_at).toLocaleString()}
                            </span>
                          </div>
                        )}
                      </div>

                      {keySessions.length > 0 && (
                        <div>
                          <h4 className="text-fg-muted font-medium mb-1">
                            Sessions ({keySessions.length})
                          </h4>
                          <table className="w-full">
                            <thead>
                              <tr className="text-left text-fg-muted border-b border-border">
                                <th className="pb-1 pr-3">ID</th>
                                <th className="pb-1 pr-3">Exchange</th>
                                <th className="pb-1 pr-3">Env</th>
                                <th className="pb-1">Status</th>
                              </tr>
                            </thead>
                            <tbody>
                              {keySessions.map((s) => (
                                <tr
                                  key={s.id}
                                  className="border-b border-border-subtle"
                                >
                                  <td className="py-1 pr-3 font-mono">
                                    {s.id}
                                  </td>
                                  <td className="py-1 pr-3">{s.exchange}</td>
                                  <td className="py-1 pr-3">
                                    {s.environment}
                                  </td>
                                  <td className="py-1">
                                    {s.suspended ? (
                                      <span className="text-red">
                                        suspended
                                      </span>
                                    ) : (
                                      <span className="text-green">active</span>
                                    )}
                                  </td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        </div>
                      )}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}
