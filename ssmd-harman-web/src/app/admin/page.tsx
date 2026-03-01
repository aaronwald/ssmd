"use client";

import { useState } from "react";
import { useMe, useAdminUsers } from "@/lib/hooks";
import type { AdminKey, AdminSession } from "@/lib/types";

export default function AdminPage() {
  const { data: me } = useMe();
  const hasAdmin = me?.scopes.includes("harman:admin") || me?.scopes.includes("*");

  if (!me) {
    return <div className="py-10 text-center text-fg-muted">Loading...</div>;
  }

  if (!hasAdmin) {
    return (
      <div className="py-10 text-center">
        <h1 className="text-xl font-bold text-fg mb-2">Admin</h1>
        <p className="text-sm text-fg-muted">
          You do not have the <code className="font-mono text-accent">harman:admin</code> scope.
        </p>
      </div>
    );
  }

  return <AdminContent />;
}

function AdminContent() {
  const { data, error } = useAdminUsers();
  const [expandedKey, setExpandedKey] = useState<string | null>(null);

  if (error) {
    return (
      <div className="space-y-6">
        <h1 className="text-xl font-bold">Admin</h1>
        <p className="text-sm text-red">Error loading admin data: {error.message}</p>
      </div>
    );
  }

  if (!data) {
    return (
      <div className="space-y-6">
        <h1 className="text-xl font-bold">Admin</h1>
        <p className="text-sm text-fg-muted">Loading...</p>
      </div>
    );
  }

  const { keys, sessions } = data;

  // Group keys by email
  const byEmail = new Map<string, AdminKey[]>();
  for (const key of keys) {
    const email = key.email || "(no email)";
    const existing = byEmail.get(email) || [];
    existing.push(key);
    byEmail.set(email, existing);
  }

  // Map sessions by key_prefix for lookup
  const sessionsByPrefix = new Map<string, AdminSession[]>();
  for (const s of sessions) {
    const existing = sessionsByPrefix.get(s.key_prefix) || [];
    existing.push(s);
    sessionsByPrefix.set(s.key_prefix, existing);
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold">Admin</h1>
        <span className="text-xs text-fg-muted">{keys.length} keys, {sessions.length} sessions</span>
      </div>

      {/* Users grouped by email */}
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
                        <span className="text-xs text-fg-muted">{isExpanded ? "▲" : "▼"}</span>
                      </div>
                    </div>
                    <div className="flex gap-2 mt-1 flex-wrap">
                      {key.scopes.map((scope) => (
                        <span key={scope} className="text-xs bg-accent/10 text-accent px-1.5 py-0.5 rounded font-mono">
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
                          <span className="font-mono text-fg">{key.rate_limit_tier || "default"}</span>
                        </div>
                        <div>
                          <span className="text-fg-muted">Feeds: </span>
                          <span className="font-mono text-fg">{key.feeds?.join(", ") || "all"}</span>
                        </div>
                        {key.expires_at && (
                          <div>
                            <span className="text-fg-muted">Expires: </span>
                            <span className="font-mono text-fg">{new Date(key.expires_at).toLocaleDateString()}</span>
                          </div>
                        )}
                        {key.last_used_at && (
                          <div>
                            <span className="text-fg-muted">Last used: </span>
                            <span className="font-mono text-fg">{new Date(key.last_used_at).toLocaleString()}</span>
                          </div>
                        )}
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
                                  <td className="py-1">
                                    {s.suspended ? (
                                      <span className="text-red">suspended</span>
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
