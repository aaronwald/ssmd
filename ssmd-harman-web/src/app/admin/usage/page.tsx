"use client";

import { useMe, useAdminUsers, useKeyUsage, useKeyRequests } from "@/lib/hooks";
import type { AdminKey, KeyUsage, KeyRequestCounts } from "@/lib/types";

export default function UsagePage() {
  const { data: me } = useMe();
  const hasAdmin = me?.scopes.includes("harman:admin") || me?.scopes.includes("*");

  if (!me) return <div className="py-10 text-center text-fg-muted">Loading...</div>;
  if (!hasAdmin) {
    return (
      <div className="py-10 text-center text-fg-muted">
        Requires <code className="font-mono text-accent">harman:admin</code> scope.
      </div>
    );
  }

  return <UsageContent />;
}

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

/** Compact relative time, e.g. "3m ago", "2h ago", "5d ago". */
function relativeTime(iso: string | null | undefined): string {
  if (!iso) return "never";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "—";
  const diffMs = Date.now() - then;
  if (diffMs < 0) return "just now";
  const sec = Math.floor(diffMs / 1000);
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  return `${day}d ago`;
}

/** Sort by last_used_at descending; keys never used sort to the bottom. */
function byLastUsedDesc(a: AdminKey, b: AdminKey): number {
  const ta = a.last_used_at ? new Date(a.last_used_at).getTime() : -Infinity;
  const tb = b.last_used_at ? new Date(b.last_used_at).getTime() : -Infinity;
  return tb - ta;
}

// ──────────────────────────────────────────────────────────────────────────────
// Content
// ──────────────────────────────────────────────────────────────────────────────

function UsageContent() {
  const { data: users, error: usersError } = useAdminUsers();
  const { data: usage, error: usageError } = useKeyUsage();
  const { data: requests, error: requestsError } = useKeyRequests();

  // The key list is the spine of the table; usage/requests are best-effort joins.
  if (usersError) return <p className="text-sm text-red">Error loading keys: {usersError.message}</p>;
  if (!users) return <p className="text-sm text-fg-muted">Loading...</p>;

  const usageByPrefix = new Map<string, KeyUsage>();
  for (const u of usage ?? []) usageByPrefix.set(u.keyPrefix, u);

  const requestsByPrefix = new Map<string, KeyRequestCounts>();
  for (const r of requests ?? []) requestsByPrefix.set(r.keyPrefix, r);

  const keys = [...users.keys].sort(byLastUsedDesc);
  const windowSeconds = (usage ?? [])[0]?.windowSeconds;
  const windowLabel = windowSeconds ? `${Math.round(windowSeconds / 60)}m` : "window";

  return (
    <div className="space-y-4">
      <div className="flex items-baseline gap-3">
        <h1 className="text-xl font-bold">Key Usage</h1>
        <span className="text-xs text-fg-muted">{keys.length} keys</span>
      </div>

      {(usageError || requestsError) && (
        <p className="text-xs text-yellow bg-yellow/10 border border-yellow/20 rounded px-3 py-2">
          Live usage counters unavailable — showing last-used times only.
          {usageError ? ` (${usageError.message})` : ""}
        </p>
      )}

      <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
        <table className="w-full text-xs">
          <thead>
            <tr className="text-left text-fg-muted border-b border-border">
              <th className="px-4 py-2 font-medium">Key</th>
              <th className="px-4 py-2 font-medium">Email</th>
              <th className="px-4 py-2 font-medium">Last used</th>
              <th className="px-4 py-2 font-medium text-right">In {windowLabel}</th>
              <th className="px-4 py-2 font-medium text-right">Rate-limit hits</th>
              <th className="px-4 py-2 font-medium text-right">Total reqs</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border-subtle">
            {keys.map((key) => {
              const u = usageByPrefix.get(key.prefix);
              const r = requestsByPrefix.get(key.prefix);
              return (
                <tr key={key.prefix} className="hover:bg-bg-surface-hover transition-colors">
                  <td className="px-4 py-2">
                    <div className="font-mono text-fg">{key.prefix}…</div>
                    {key.name && <div className="text-fg-muted">{key.name}</div>}
                  </td>
                  <td className="px-4 py-2 text-fg-muted">{key.email || "—"}</td>
                  <td className="px-4 py-2">
                    {key.last_used_at ? (
                      <span className="text-fg" title={new Date(key.last_used_at).toLocaleString()}>
                        {relativeTime(key.last_used_at)}
                      </span>
                    ) : (
                      <span className="text-fg-subtle">never</span>
                    )}
                  </td>
                  <td className="px-4 py-2 text-right font-mono">
                    {u ? (
                      <span className={u.requestsInWindow >= u.limit ? "text-red" : "text-fg"}>
                        {u.requestsInWindow} / {u.limit}
                      </span>
                    ) : (
                      <span className="text-fg-subtle">—</span>
                    )}
                  </td>
                  <td className="px-4 py-2 text-right font-mono">
                    {u ? (
                      <span className={u.rateLimitHits > 0 ? "text-yellow" : "text-fg-subtle"}>
                        {u.rateLimitHits}
                      </span>
                    ) : (
                      <span className="text-fg-subtle">—</span>
                    )}
                  </td>
                  <td className="px-4 py-2 text-right font-mono text-fg">
                    {r ? r.totalRequests.toLocaleString() : <span className="text-fg-subtle">—</span>}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <p className="text-[11px] text-fg-subtle">
        &ldquo;In {windowLabel}&rdquo; and rate-limit hits are from the rolling rate-limit window.
        &ldquo;Total reqs&rdquo; is counted since the last data-ts pod restart, not all-time.
      </p>
    </div>
  );
}
