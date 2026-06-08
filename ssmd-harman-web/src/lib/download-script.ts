import type { SignedDataFile } from "./types";

/** Human-readable byte size, e.g. 179799 -> "175.6 KB". */
export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  const val = bytes / Math.pow(1024, i);
  return `${val.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

/**
 * Build a bash curl script that downloads every file into <feed>/<date>/.
 * `expiresAt` (ISO 8601, from the download response) is surfaced in the header
 * so it is obvious the signed URLs are short-lived (max 12h).
 */
export function buildDownloadScript(
  feed: string,
  date: string,
  files: SignedDataFile[],
  expiresAt: string | null,
): string {
  const header = [
    "#!/usr/bin/env bash",
    `# ssmd daily files — ${feed} ${date}${expiresAt ? ` — URLs expire ${expiresAt}` : ""}`,
    "set -euo pipefail",
    `mkdir -p ${feed}/${date}`,
  ];
  const commands = files.map(
    (f) => `curl -fsSL -o ${feed}/${date}/${f.name} '${f.signedUrl}'`,
  );
  return [...header, ...commands].join("\n") + "\n";
}

/** Concatenate per-feed scripts into one day-wide script (shared shebang/header). */
export function buildDayScript(
  date: string,
  perFeed: { feed: string; files: SignedDataFile[]; expiresAt: string | null }[],
): string {
  const earliestExpiry = perFeed
    .map((p) => p.expiresAt)
    .filter((e): e is string => e !== null)
    .sort()[0] ?? null;
  const header = [
    "#!/usr/bin/env bash",
    `# ssmd daily files — all feeds ${date}${earliestExpiry ? ` — URLs expire by ${earliestExpiry}` : ""}`,
    "set -euo pipefail",
  ];
  const body: string[] = [];
  for (const { feed, files } of perFeed) {
    if (files.length === 0) continue;
    body.push(`mkdir -p ${feed}/${date}`);
    for (const f of files) {
      body.push(`curl -fsSL -o ${feed}/${date}/${f.name} '${f.signedUrl}'`);
    }
  }
  return [...header, ...body].join("\n") + "\n";
}
