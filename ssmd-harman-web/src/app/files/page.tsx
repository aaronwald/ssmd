"use client";

import { useMemo, useState } from "react";
import { useMe, useDayFiles, useDataCatalog } from "@/lib/hooks";
import { getDataDownload } from "@/lib/api";
import { formatBytes, buildDownloadScript, buildDayScript } from "@/lib/download-script";
import type { DayFeed, SignedDataFile } from "@/lib/types";

function todayUtc(): string {
  return new Date().toISOString().slice(0, 10);
}

export default function FilesPage() {
  const { data: me } = useMe();
  const canReadData =
    me?.scopes.includes("datasets:read") ||
    me?.scopes.includes("harman:admin") ||
    me?.scopes.includes("*");

  if (!me) return <div className="py-10 text-center text-fg-muted">Loading...</div>;
  if (!canReadData) {
    return (
      <div className="py-10 text-center text-fg-muted">
        Requires <code className="font-mono text-accent">datasets:read</code> scope.
      </div>
    );
  }
  return <FilesContent />;
}

function FilesContent() {
  const { data: catalog } = useDataCatalog();
  const dateMax = useMemo(
    () => catalog?.feeds.reduce<string | null>((max, f) => (!max || f.dateMax > max ? f.dateMax : max), null) ?? null,
    [catalog],
  );
  const dateMin = useMemo(
    () => catalog?.feeds.reduce<string | null>((min, f) => (!min || f.dateMin < min ? f.dateMin : min), null) ?? null,
    [catalog],
  );

  // Until the user picks a date, default to the catalog's latest available date
  // (today's UTC date is usually empty — parquet-gen runs every 6h).
  const [date, setDate] = useState<string | null>(null);
  const effectiveDate = date ?? dateMax ?? todayUtc();
  const { data, error, isLoading } = useDayFiles(effectiveDate);

  // Lazy signing cache: feed -> signed files (+ shared expiry).
  const [signed, setSigned] = useState<Record<string, { files: SignedDataFile[]; expiresAt: string | null }>>({});
  const [signing, setSigning] = useState<Record<string, boolean>>({});
  const [signErr, setSignErr] = useState<Record<string, string>>({});

  function stepDay(delta: number) {
    const d = new Date(`${effectiveDate}T00:00:00Z`);
    d.setUTCDate(d.getUTCDate() + delta);
    setDate(d.toISOString().slice(0, 10));
    setSigned({});
    setSignErr({});
  }

  async function signFeed(feed: string): Promise<{ files: SignedDataFile[]; expiresAt: string | null } | null> {
    const cached = signed[feed];
    if (cached) return cached;
    setSigning((s) => ({ ...s, [feed]: true }));
    setSignErr((e) => ({ ...e, [feed]: "" }));
    try {
      const res = await getDataDownload(feed, effectiveDate);
      const entry = { files: res.files, expiresAt: res.files[0]?.expiresAt ?? null };
      setSigned((s) => ({ ...s, [feed]: entry }));
      return entry;
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Failed to sign URLs";
      setSignErr((e) => ({ ...e, [feed]: msg }));
      return null;
    } finally {
      setSigning((s) => ({ ...s, [feed]: false }));
    }
  }

  async function downloadFile(feed: string, name: string) {
    const entry = await signFeed(feed);
    const file = entry?.files.find((f) => f.name === name);
    if (file) triggerDownload(file.signedUrl);
  }

  async function downloadAll(feed: string, fileCount: number) {
    const entry = await signFeed(feed);
    if (!entry) return;
    if (fileCount > 3) {
      // Browsers block programmatic multi-file downloads past the first few —
      // give the script instead so nothing is silently dropped.
      await copyScript(feed);
      return;
    }
    for (const f of entry.files) triggerDownload(f.signedUrl);
  }

  async function copyScript(feed: string) {
    const entry = await signFeed(feed);
    if (!entry) return;
    const script = buildDownloadScript(feed, effectiveDate, entry.files, entry.expiresAt);
    await navigator.clipboard.writeText(script);
  }

  async function copyDayScript(feeds: DayFeed[]) {
    const perFeed = [];
    for (const f of feeds) {
      if (f.fileCount === 0) continue;
      const entry = await signFeed(f.feed);
      if (entry) perFeed.push({ feed: f.feed, files: entry.files, expiresAt: entry.expiresAt });
    }
    await navigator.clipboard.writeText(buildDayScript(effectiveDate, perFeed));
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-3 flex-wrap">
        <h1 className="text-xl font-bold">Daily Files</h1>
        <div className="flex items-center gap-1">
          <button onClick={() => stepDay(-1)} className="px-2 py-1 text-sm border border-border rounded hover:bg-bg-raised" aria-label="Previous day">‹</button>
          <input
            type="date"
            value={effectiveDate}
            min={dateMin ?? undefined}
            max={dateMax ?? undefined}
            onChange={(e) => { setDate(e.target.value); setSigned({}); setSignErr({}); }}
            className="bg-bg-raised border border-border rounded px-2 py-1 text-sm font-mono"
          />
          <button onClick={() => stepDay(1)} className="px-2 py-1 text-sm border border-border rounded hover:bg-bg-raised" aria-label="Next day">›</button>
        </div>
        {data && data.feeds.some((f) => f.fileCount > 0) && (
          <button onClick={() => copyDayScript(data.feeds)} className="ml-auto px-3 py-1 text-sm text-accent border border-border rounded hover:bg-bg-raised">
            Copy script (all feeds)
          </button>
        )}
      </div>

      {isLoading && <p className="text-sm text-fg-muted">Loading files...</p>}
      {error && <p className="text-sm text-red">Error loading files: {error.message}</p>}

      {data && data.feeds.map((feed) => (
        <div key={feed.feed} className="bg-bg-raised border border-border rounded-lg overflow-hidden">
          <div className="px-4 py-3 border-b border-border flex items-center gap-3 flex-wrap">
            <div className="flex flex-col min-w-0">
              <span className="text-sm font-medium text-fg">{feed.feed}</span>
              {feed.description && <span className="text-xs text-fg-muted">{feed.description}</span>}
            </div>
            <span className="text-xs text-fg-muted">{feed.stream}</span>
            <span className="text-xs text-fg-muted">{feed.fileCount} file{feed.fileCount !== 1 ? "s" : ""} · {formatBytes(feed.totalBytes)}</span>
            {feed.fileCount > 0 && (
              <div className="ml-auto flex items-center gap-2">
                <button onClick={() => copyScript(feed.feed)} disabled={signing[feed.feed]} className="px-2 py-1 text-xs text-accent border border-border rounded hover:bg-bg disabled:opacity-50">
                  {signing[feed.feed] ? "Signing..." : "Copy script"}
                </button>
                <button onClick={() => downloadAll(feed.feed, feed.fileCount)} disabled={signing[feed.feed]} className="px-2 py-1 text-xs text-accent border border-border rounded hover:bg-bg disabled:opacity-50">
                  Download all
                </button>
              </div>
            )}
          </div>

          {signErr[feed.feed] && <p className="px-4 py-2 text-xs text-red">{signErr[feed.feed]}</p>}

          {feed.fileCount === 0 ? (
            <p className="px-4 py-3 text-xs text-fg-muted">No files for this date.</p>
          ) : (
            <div className="divide-y divide-border-subtle">
              {feed.files.map((file) => (
                <div key={file.name} className="px-4 py-2 flex items-center gap-3 text-sm">
                  <button onClick={() => downloadFile(feed.feed, file.name)} className="font-mono text-accent hover:underline text-left">
                    {file.name}
                  </button>
                  <span className="text-xs px-1.5 py-0.5 rounded bg-bg text-fg-muted">{file.type}</span>
                  <span className="text-xs text-fg-muted">{file.hour}</span>
                  <span className="ml-auto text-xs text-fg-muted">{formatBytes(file.bytes)}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function triggerDownload(url: string) {
  const a = document.createElement("a");
  a.href = url;
  a.rel = "noopener";
  document.body.appendChild(a);
  a.click();
  a.remove();
}
