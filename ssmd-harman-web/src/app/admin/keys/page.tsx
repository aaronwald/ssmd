"use client";

import { useState } from "react";
import { useMe, useAdminUsers, useCreateKey, useRotateWelcome } from "@/lib/hooks";
import type { AdminKey, AdminSession, CreateKeyRequest } from "@/lib/types";

const KNOWN_FEEDS = ["hols", "kalshi", "kraken-futures", "kraken-spot", "polymarket"] as const;
const DEFAULT_SCOPES = ["datasets:read"];
const DEFAULT_DATE_START = "2026-01-01";
const DEFAULT_DATE_END = "2099-12-31";

export default function KeysPage() {
  const { data: me } = useMe();
  const hasAdmin = me?.scopes.includes("harman:admin") || me?.scopes.includes("*");

  if (!me) return <div className="py-10 text-center text-fg-muted">Loading...</div>;
  if (!hasAdmin) return <div className="py-10 text-center text-fg-muted">Requires <code className="font-mono text-accent">harman:admin</code> scope.</div>;

  return <KeysContent />;
}

// ──────────────────────────────────────────────────────────────────────────────
// Create Key Form
// ──────────────────────────────────────────────────────────────────────────────

interface CreateFormState {
  name: string;
  userEmail: string;
  feeds: Set<string>;
  scopes: string;
  dateRangeStart: string;
  dateRangeEnd: string;
  sendWelcome: boolean;
  recipient: string;
}

function CreateKeyForm({ onCreated }: { onCreated: () => void }) {
  const createKey = useCreateKey();

  const [form, setForm] = useState<CreateFormState>({
    name: "",
    userEmail: "",
    feeds: new Set(KNOWN_FEEDS),
    scopes: DEFAULT_SCOPES.join(", "),
    dateRangeStart: DEFAULT_DATE_START,
    dateRangeEnd: DEFAULT_DATE_END,
    sendWelcome: true,
    recipient: "",
  });

  const [successMsg, setSuccessMsg] = useState<string | null>(null);

  function toggleFeed(feed: string) {
    setForm((prev) => {
      const next = new Set(prev.feeds);
      if (next.has(feed)) {
        next.delete(feed);
      } else {
        next.add(feed);
      }
      return { ...prev, feeds: next };
    });
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setSuccessMsg(null);
    createKey.reset();

    const scopes = form.scopes
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);

    const payload: CreateKeyRequest = {
      name: form.name.trim(),
      userEmail: form.userEmail.trim(),
      scopes,
      allowedFeeds: Array.from(form.feeds),
      dateRangeStart: form.dateRangeStart,
      dateRangeEnd: form.dateRangeEnd,
      sendWelcome: form.sendWelcome,
      recipient: form.sendWelcome ? (form.recipient.trim() || form.userEmail.trim()) : undefined,
    };

    try {
      const result = await createKey.trigger(payload);
      let msg = `Key ${result.prefix} created.`;
      if (result.welcome) {
        if (result.welcome.sent) {
          msg += ` Welcome email sent to ${payload.recipient || payload.userEmail}.`;
        } else if (result.welcome.error) {
          msg += ` Welcome email failed: ${result.welcome.error}`;
        }
      }
      setSuccessMsg(msg);
      setForm((prev) => ({
        ...prev,
        name: "",
        userEmail: "",
        recipient: "",
        sendWelcome: true,
      }));
      onCreated();
    } catch {
      // error already stored in createKey.error
    }
  }

  return (
    <form onSubmit={handleSubmit} className="bg-bg-raised border border-border rounded-lg p-4 space-y-4">
      <h2 className="text-sm font-semibold text-fg">New API Key</h2>

      {successMsg && (
        <p className="text-xs text-green bg-green/10 border border-green/20 rounded px-3 py-2">{successMsg}</p>
      )}
      {createKey.error && (
        <p className="text-xs text-red bg-red/10 border border-red/20 rounded px-3 py-2">{createKey.error}</p>
      )}

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <div className="space-y-1">
          <label htmlFor="key-email" className="text-xs text-fg-muted">User email <span className="text-red">*</span></label>
          <input
            id="key-email"
            type="email"
            required
            value={form.userEmail}
            onChange={(e) => setForm((prev) => ({
              ...prev,
              userEmail: e.target.value,
              recipient: prev.sendWelcome && prev.recipient === prev.userEmail ? e.target.value : prev.recipient,
            }))}
            placeholder="user@example.com"
            className="w-full bg-bg border border-border rounded px-2 py-1.5 text-xs text-fg placeholder:text-fg-muted focus:outline-none focus:ring-1 focus:ring-accent"
          />
        </div>

        <div className="space-y-1">
          <label htmlFor="key-name" className="text-xs text-fg-muted">Key name <span className="text-red">*</span></label>
          <input
            id="key-name"
            type="text"
            required
            value={form.name}
            onChange={(e) => setForm((prev) => ({ ...prev, name: e.target.value }))}
            placeholder="e.g. analytics-prod"
            className="w-full bg-bg border border-border rounded px-2 py-1.5 text-xs text-fg placeholder:text-fg-muted focus:outline-none focus:ring-1 focus:ring-accent"
          />
        </div>

        <div className="space-y-1">
          <label htmlFor="key-scopes" className="text-xs text-fg-muted">Scopes (comma-separated)</label>
          <input
            id="key-scopes"
            type="text"
            value={form.scopes}
            onChange={(e) => setForm((prev) => ({ ...prev, scopes: e.target.value }))}
            className="w-full bg-bg border border-border rounded px-2 py-1.5 text-xs font-mono text-fg placeholder:text-fg-muted focus:outline-none focus:ring-1 focus:ring-accent"
          />
        </div>

        <div className="space-y-1">
          <span className="text-xs text-fg-muted block mb-1">Allowed feeds</span>
          <div className="flex flex-wrap gap-2">
            {KNOWN_FEEDS.map((feed) => (
              <label key={feed} className="flex items-center gap-1.5 cursor-pointer">
                <input
                  type="checkbox"
                  checked={form.feeds.has(feed)}
                  onChange={() => toggleFeed(feed)}
                  className="accent-accent"
                />
                <span className="text-xs font-mono text-fg">{feed}</span>
              </label>
            ))}
          </div>
        </div>

        <div className="space-y-1">
          <label htmlFor="key-date-start" className="text-xs text-fg-muted">Date range start</label>
          <input
            id="key-date-start"
            type="date"
            value={form.dateRangeStart}
            onChange={(e) => setForm((prev) => ({ ...prev, dateRangeStart: e.target.value }))}
            className="w-full bg-bg border border-border rounded px-2 py-1.5 text-xs text-fg focus:outline-none focus:ring-1 focus:ring-accent"
          />
        </div>

        <div className="space-y-1">
          <label htmlFor="key-date-end" className="text-xs text-fg-muted">Date range end</label>
          <input
            id="key-date-end"
            type="date"
            value={form.dateRangeEnd}
            onChange={(e) => setForm((prev) => ({ ...prev, dateRangeEnd: e.target.value }))}
            className="w-full bg-bg border border-border rounded px-2 py-1.5 text-xs text-fg focus:outline-none focus:ring-1 focus:ring-accent"
          />
        </div>
      </div>

      <div className="space-y-2">
        <label className="flex items-center gap-2 cursor-pointer">
          <input
            type="checkbox"
            checked={form.sendWelcome}
            onChange={(e) => setForm((prev) => ({ ...prev, sendWelcome: e.target.checked }))}
            className="accent-accent"
          />
          <span className="text-xs text-fg">Send welcome email with API key</span>
        </label>

        {form.sendWelcome && (
          <div className="space-y-1 ml-5">
            <label htmlFor="key-recipient" className="text-xs text-fg-muted">Recipient (defaults to user email)</label>
            <input
              id="key-recipient"
              type="email"
              value={form.recipient}
              onChange={(e) => setForm((prev) => ({ ...prev, recipient: e.target.value }))}
              placeholder={form.userEmail || "recipient@example.com"}
              className="w-full bg-bg border border-border rounded px-2 py-1.5 text-xs text-fg placeholder:text-fg-muted focus:outline-none focus:ring-1 focus:ring-accent"
            />
          </div>
        )}
      </div>

      <div className="flex justify-end">
        <button
          type="submit"
          disabled={createKey.loading}
          className="px-3 py-1.5 text-xs font-medium bg-accent text-bg rounded hover:bg-accent/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {createKey.loading ? "Creating..." : "Create key"}
        </button>
      </div>
    </form>
  );
}

// ──────────────────────────────────────────────────────────────────────────────
// Per-key Rotate & Send Welcome panel
// ──────────────────────────────────────────────────────────────────────────────

function RotatePanel({ keyPrefix, defaultEmail }: { keyPrefix: string; defaultEmail: string }) {
  const rotate = useRotateWelcome();
  const [confirmStep, setConfirmStep] = useState(false);
  const [recipientOverride, setRecipientOverride] = useState("");
  const [resultMsg, setResultMsg] = useState<string | null>(null);

  function handleRequestConfirm() {
    rotate.reset();
    setResultMsg(null);
    setConfirmStep(true);
  }

  function handleCancel() {
    setConfirmStep(false);
    rotate.reset();
  }

  async function handleConfirm() {
    const recipient = recipientOverride.trim() || defaultEmail;
    try {
      const result = await rotate.trigger(keyPrefix, recipient);
      let msg = `Key rotated (new prefix: ${result.prefix}).`;
      if (result.welcome.sent) {
        msg += ` Welcome email sent to ${recipient}.`;
      } else {
        msg += ` Welcome email failed: ${result.welcome.error ?? "unknown error"}`;
      }
      setResultMsg(msg);
      setConfirmStep(false);
    } catch {
      // error stored in rotate.error
      setConfirmStep(false);
    }
  }

  if (resultMsg) {
    return (
      <p className="text-xs text-green bg-green/10 border border-green/20 rounded px-3 py-2 mt-2">{resultMsg}</p>
    );
  }

  if (rotate.error) {
    return (
      <div className="mt-2 space-y-1">
        <p className="text-xs text-red bg-red/10 border border-red/20 rounded px-3 py-2">{rotate.error}</p>
        <button
          onClick={() => rotate.reset()}
          className="text-xs text-fg-muted underline"
        >
          Dismiss
        </button>
      </div>
    );
  }

  if (!confirmStep) {
    return (
      <button
        onClick={handleRequestConfirm}
        className="mt-2 px-3 py-1 text-xs font-medium border border-border rounded text-fg hover:bg-bg-surface-hover transition-colors"
      >
        Rotate &amp; send welcome
      </button>
    );
  }

  return (
    <div className="mt-2 space-y-2 bg-red/5 border border-red/20 rounded p-3">
      <p className="text-xs text-red font-medium">This invalidates the current secret. Continue?</p>
      <div className="space-y-1">
        <label htmlFor={`rotate-recipient-${keyPrefix}`} className="text-xs text-fg-muted">
          Send welcome to (defaults to {defaultEmail || "key email"})
        </label>
        <input
          id={`rotate-recipient-${keyPrefix}`}
          type="email"
          value={recipientOverride}
          onChange={(e) => setRecipientOverride(e.target.value)}
          placeholder={defaultEmail || "recipient@example.com"}
          className="w-full bg-bg border border-border rounded px-2 py-1.5 text-xs text-fg placeholder:text-fg-muted focus:outline-none focus:ring-1 focus:ring-accent"
        />
      </div>
      <div className="flex gap-2">
        <button
          onClick={handleConfirm}
          disabled={rotate.loading}
          className="px-3 py-1 text-xs font-medium bg-red text-bg rounded hover:bg-red/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {rotate.loading ? "Rotating..." : "Yes, rotate & send"}
        </button>
        <button
          onClick={handleCancel}
          disabled={rotate.loading}
          className="px-3 py-1 text-xs font-medium border border-border rounded text-fg hover:bg-bg-surface-hover transition-colors disabled:opacity-50"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────────────────
// Main content
// ──────────────────────────────────────────────────────────────────────────────

function KeysContent() {
  const { data, error } = useAdminUsers();
  const [expandedKey, setExpandedKey] = useState<string | null>(null);
  const [showCreateForm, setShowCreateForm] = useState(false);

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
    <div className="space-y-6">
      <div className="flex items-center gap-3">
        <h1 className="text-xl font-bold">API Keys</h1>
        <span className="text-xs text-fg-muted">{keys.length} keys, {sessions.length} sessions</span>
        <button
          onClick={() => setShowCreateForm((v) => !v)}
          className="ml-auto px-3 py-1.5 text-xs font-medium bg-accent text-bg rounded hover:bg-accent/90 transition-colors"
        >
          {showCreateForm ? "Cancel" : "+ New key"}
        </button>
      </div>

      {showCreateForm && (
        <CreateKeyForm onCreated={() => setShowCreateForm(false)} />
      )}

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

                      <RotatePanel keyPrefix={key.prefix} defaultEmail={key.email || ""} />
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
