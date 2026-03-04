"use client";

import { useState, useMemo } from "react";
import { useMe, useAdminUsers, useHarmanSessions, useExchangeAudit } from "@/lib/hooks";
import type { AdminKey, AdminSession, ExchangeAuditEntry } from "@/lib/types";

type SystemTab = "keys" | "audit";

export default function SystemPage() {
  const { data: me } = useMe();
  const hasAdmin = me?.scopes.includes("harman:admin") || me?.scopes.includes("*");

  if (!me) return <div className="py-10 text-center text-fg-muted">Loading...</div>;
  if (!hasAdmin) return <div className="py-10 text-center text-fg-muted">Requires <code className="font-mono text-accent">harman:admin</code> scope.</div>;

  return <SystemContent />;
}

function SystemContent() {
  const [tab, setTab] = useState<SystemTab>("keys");

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold">System</h1>
        <div className="flex gap-1">
          {(["keys", "audit"] as const).map((t) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={`rounded-md px-3 py-1 text-sm transition-colors ${
                tab === t ? "bg-accent text-fg" : "text-fg-muted hover:text-fg"
              }`}
            >
              {t === "keys" ? "API Keys" : "Exchange Audit"}
            </button>
          ))}
        </div>
      </div>

      {tab === "keys" && <KeysSection />}
      {tab === "audit" && <AuditSection />}
    </div>
  );
}

/* ─── API Keys ─── */

function KeysSection() {
  const { data, error } = useAdminUsers();
  const [expandedKey, setExpandedKey] = useState<string | null>(null);

  if (error) return <p className="text-sm text-red">Error loading admin data: {error.message}</p>;
  if (!data) return <p className="text-sm text-fg-muted">Loading...</p>;

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
        <span className="text-xs text-fg-muted">{keys.length} keys, {sessions.length} sessions</span>
      </div>

      {Array.from(byEmail.entries()).map(([email, userKeys]) => (
        <div key={email} className="bg-bg-raised border border-border rounded-lg overflow-hidden">
          <div className="px-4 py-3 border-b border-border">
            <span className="text-sm font-medium text-fg">{email}</span>
            <span className="text-xs text-fg-muted ml-2">{userKeys.length} key{userKeys.length !== 1 ? "s" : ""}</span>
          </div>

          <div className="divide-y divide-border-subtle">
            {userKeys.map((key) => {
              const isExpanded = expandedKey === key.prefix;
              const keySessions = sessionsByPrefix.get(key.prefix) || [];

              return (
                <div key={key.prefix}>
                  <button
                    onClick={() => setExpandedKey(isExpanded ? null : key.prefix)}
                    className="w-full text-left px-4 py-2 hover:bg-bg-surface-hover transition-colors"
                  >
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <span className="font-mono text-xs text-fg">{key.prefix}...</span>
                        {key.name && <span className="text-xs text-fg-muted">{key.name}</span>}
                      </div>
                      <div className="flex items-center gap-2">
                        {keySessions.some((s) => s.suspended) && (
                          <span className="text-xs bg-red/15 text-red px-2 py-0.5 rounded">suspended</span>
                        )}
                        <span className="text-xs text-fg-muted">{isExpanded ? "\u25B2" : "\u25BC"}</span>
                      </div>
                    </div>
                    <div className="flex gap-2 mt-1 flex-wrap">
                      {key.scopes.map((scope) => (
                        <span key={scope} className="text-xs bg-accent/10 text-accent px-1.5 py-0.5 rounded font-mono">{scope}</span>
                      ))}
                    </div>
                  </button>

                  {isExpanded && (
                    <div className="px-4 py-3 bg-bg text-xs space-y-3">
                      <div className="grid grid-cols-2 gap-2">
                        <div><span className="text-fg-muted">Tier: </span><span className="font-mono text-fg">{key.rate_limit_tier || "default"}</span></div>
                        <div><span className="text-fg-muted">Feeds: </span><span className="font-mono text-fg">{key.feeds?.join(", ") || "all"}</span></div>
                        {key.expires_at && <div><span className="text-fg-muted">Expires: </span><span className="font-mono text-fg">{new Date(key.expires_at).toLocaleDateString()}</span></div>}
                        {key.last_used_at && <div><span className="text-fg-muted">Last used: </span><span className="font-mono text-fg">{new Date(key.last_used_at).toLocaleString()}</span></div>}
                      </div>

                      {keySessions.length > 0 && (
                        <div>
                          <h4 className="text-fg-muted font-medium mb-1">Sessions ({keySessions.length})</h4>
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
                                <tr key={s.id} className="border-b border-border-subtle">
                                  <td className="py-1 pr-3 font-mono">{s.id}</td>
                                  <td className="py-1 pr-3">{s.exchange}</td>
                                  <td className="py-1 pr-3">{s.environment}</td>
                                  <td className="py-1">{s.suspended ? <span className="text-red">suspended</span> : <span className="text-green">active</span>}</td>
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

/* ─── Exchange Audit ─── */

function AuditSection() {
  const { data: sessions } = useHarmanSessions();
  const [selected, setSelected] = useState<{ id: number; instance: string } | null>(null);

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <select
          value={selected ? `${selected.instance}:${selected.id}` : ""}
          onChange={(e) => {
            if (!e.target.value) { setSelected(null); return; }
            const [inst, id] = e.target.value.split(":");
            setSelected({ instance: inst, id: Number(id) });
          }}
          className="rounded-md border border-border bg-bg-surface px-3 py-1 text-sm text-fg focus:border-accent focus:outline-none"
        >
          <option value="">Select session...</option>
          {sessions?.map((s) => (
            <option key={`${s.instance}-${s.id}`} value={`${s.instance}:${s.id}`}>
              #{s.id} — {s.instance} — {s.exchange} ({s.environment})
              {s.display_name ? ` — ${s.display_name}` : ""}
            </option>
          ))}
        </select>
      </div>

      {selected && <AuditLogTable sessionId={selected.id} instance={selected.instance} />}
      {!selected && <p className="text-sm text-fg-muted">Select a session to view exchange audit entries.</p>}
    </div>
  );
}

function AuditLogTable({ sessionId, instance }: { sessionId: number; instance?: string }) {
  const { data: audit, error } = useExchangeAudit(sessionId, instance);
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

  const categories = useMemo(() => !audit ? [] : [...new Set(audit.map((a) => a.category))].sort(), [audit]);
  const outcomes = useMemo(() => !audit ? [] : [...new Set(audit.map((a) => a.outcome))].sort(), [audit]);

  if (error) return <p className="text-sm text-red">Error loading audit: {error.message}</p>;

  return (
    <div className="space-y-3">
      <div className="flex gap-2">
        <select value={categoryFilter} onChange={(e) => setCategoryFilter(e.target.value)}
          className="rounded-md border border-border bg-bg-surface px-2 py-1 text-xs text-fg focus:border-accent focus:outline-none">
          <option value="">All categories</option>
          {categories.map((c) => <option key={c} value={c}>{c}</option>)}
        </select>
        <select value={outcomeFilter} onChange={(e) => setOutcomeFilter(e.target.value)}
          className="rounded-md border border-border bg-bg-surface px-2 py-1 text-xs text-fg focus:border-accent focus:outline-none">
          <option value="">All outcomes</option>
          {outcomes.map((o) => <option key={o} value={o}>{o}</option>)}
        </select>
        {audit && <span className="text-xs text-fg-muted self-center">{filtered?.length} / {audit.length} entries</span>}
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
              {filtered && filtered.length > 0 ? filtered.map((a) => (
                <AuditRow key={a.id} entry={a} />
              )) : (
                <tr><td colSpan={9} className="px-4 py-6 text-center text-fg-subtle text-sm">{filtered ? "No audit entries" : "Loading..."}</td></tr>
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
        {new Date(a.created_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}
      </td>
      <td className="px-3 py-1.5">
        <span className={`inline-block rounded px-1.5 py-0.5 font-mono ${categoryColors[a.category] || "bg-fg-subtle/15 text-fg-subtle"}`}>{a.category}</span>
      </td>
      <td className="px-3 py-1.5 font-mono text-fg">{a.action}</td>
      <td className="px-3 py-1.5 font-mono text-fg-muted">{a.order_id ?? "—"}</td>
      <td className="px-3 py-1.5 text-fg-muted truncate max-w-[200px]">{a.endpoint ?? "—"}</td>
      <td className="px-3 py-1.5 text-right font-mono">
        {a.status_code ? <span className={a.status_code >= 400 ? "text-red" : "text-green"}>{a.status_code}</span> : "—"}
      </td>
      <td className="px-3 py-1.5 text-right font-mono text-fg-muted">{a.duration_ms != null ? `${a.duration_ms}ms` : "—"}</td>
      <td className="px-3 py-1.5"><span className={outcomeColors[a.outcome] || "text-fg-muted"}>{a.outcome}</span></td>
      <td className="px-3 py-1.5 text-red truncate max-w-[200px]" title={a.error_msg ?? undefined}>{a.error_msg ?? ""}</td>
    </tr>
  );
}
