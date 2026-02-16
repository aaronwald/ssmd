/**
 * ssmd audit-email - Send daily data access audit report via email
 *
 * Reads data_access_log and api_keys from PostgreSQL, builds an HTML email,
 * and sends via SMTP. Designed to run as a daily CronJob.
 */
import { getDb, closeDb } from "../../lib/db/mod.ts";
import { listRecentAccess } from "../../lib/db/accesslog.ts";
import { apiKeys } from "../../lib/db/schema.ts";
import { isNull, isNotNull, gte, and, lte } from "drizzle-orm";
import nodemailer from "nodemailer";

export async function handleAuditEmail(): Promise<void> {
  const host = Deno.env.get("SMTP_HOST") ?? "smtp.gmail.com";
  const port = Number(Deno.env.get("SMTP_PORT") ?? "587");
  const user = Deno.env.get("SMTP_USER");
  const pass = Deno.env.get("SMTP_PASS");
  const to = Deno.env.get("SMTP_TO");

  if (!user || !pass || !to) {
    console.error("SMTP_USER, SMTP_PASS, and SMTP_TO are required");
    return;
  }

  const db = getDb();

  try {
    // Yesterday UTC
    const now = new Date();
    const yesterday = new Date(now);
    yesterday.setUTCDate(yesterday.getUTCDate() - 1);
    yesterday.setUTCHours(0, 0, 0, 0);

    const todayStart = new Date(now);
    todayStart.setUTCHours(0, 0, 0, 0);

    // 1. Downloads in last 24h
    const recentAccess = await listRecentAccess(db, yesterday);

    // 2. Active keys (not revoked, not expired)
    const activeKeys = await db
      .select()
      .from(apiKeys)
      .where(
        and(
          isNull(apiKeys.revokedAt),
          // Either no expiration or expires in the future
        ),
      );

    const nowDate = new Date();
    const activeFiltered = activeKeys.filter(
      (k) => !k.expiresAt || new Date(k.expiresAt) > nowDate,
    );

    // 3. Recently expired keys (last 7 days)
    const sevenDaysAgo = new Date(now);
    sevenDaysAgo.setUTCDate(sevenDaysAgo.getUTCDate() - 7);

    const expiredKeys = await db
      .select()
      .from(apiKeys)
      .where(
        and(
          isNull(apiKeys.revokedAt),
          isNotNull(apiKeys.expiresAt),
          lte(apiKeys.expiresAt, nowDate),
          gte(apiKeys.expiresAt, sevenDaysAgo),
        ),
      );

    // Build HTML email
    const dateStr = yesterday.toISOString().slice(0, 10);
    const html = buildEmailHtml(dateStr, recentAccess, activeFiltered, expiredKeys);

    // Send email
    const transporter = nodemailer.createTransport({
      host,
      port,
      secure: false,
      auth: { user, pass },
    });

    await transporter.sendMail({
      from: user,
      to,
      subject: `[SSMD] Data Access Audit — ${dateStr}`,
      html,
    });

    console.log(`Audit email sent to ${to} for ${dateStr}`);
    console.log(`  Downloads: ${recentAccess.length}`);
    console.log(`  Active keys: ${activeFiltered.length}`);
    console.log(`  Recently expired: ${expiredKeys.length}`);
  } finally {
    await closeDb();
  }
}

interface AccessEntry {
  keyPrefix: string;
  userEmail: string;
  feed: string;
  dateFrom: string;
  dateTo: string;
  msgType: string | null;
  filesCount: number;
  createdAt: Date;
}

interface KeyEntry {
  keyPrefix: string;
  userEmail: string;
  name: string;
  scopes: string[];
  createdAt: Date;
  expiresAt: Date | null;
}

function buildEmailHtml(
  dateStr: string,
  downloads: AccessEntry[],
  activeKeys: KeyEntry[],
  expiredKeys: KeyEntry[],
): string {
  const styles = `
    <style>
      body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 20px; background: #f5f5f5; }
      .container { max-width: 700px; margin: 0 auto; background: #fff; border-radius: 8px; padding: 24px; }
      h1 { color: #1a1a1a; font-size: 20px; border-bottom: 2px solid #e0e0e0; padding-bottom: 8px; }
      h2 { color: #333; font-size: 16px; margin-top: 24px; }
      table { width: 100%; border-collapse: collapse; font-size: 13px; }
      th { background: #f8f8f8; text-align: left; padding: 8px; border-bottom: 2px solid #ddd; }
      td { padding: 6px 8px; border-bottom: 1px solid #eee; }
      .empty { color: #999; font-style: italic; padding: 12px; }
      .badge { display: inline-block; padding: 2px 8px; border-radius: 4px; font-size: 11px; }
      .badge-active { background: #e6f4ea; color: #1e7e34; }
      .badge-expired { background: #fce8e6; color: #c5221f; }
      .footer { margin-top: 24px; font-size: 11px; color: #999; border-top: 1px solid #eee; padding-top: 12px; }
    </style>
  `;

  // Downloads section
  let downloadsHtml: string;
  if (downloads.length === 0) {
    downloadsHtml = '<p class="empty">No data downloads in the last 24 hours.</p>';
  } else {
    const rows = downloads.map((d) => `
      <tr>
        <td>${escapeHtml(d.userEmail)}</td>
        <td>${escapeHtml(d.feed)}</td>
        <td>${d.dateFrom} — ${d.dateTo}</td>
        <td>${d.msgType ?? "all"}</td>
        <td>${d.filesCount}</td>
        <td>${escapeHtml(d.keyPrefix)}</td>
      </tr>
    `).join("");

    downloadsHtml = `
      <table>
        <tr><th>User</th><th>Feed</th><th>Date Range</th><th>Type</th><th>Files</th><th>Key</th></tr>
        ${rows}
      </table>
    `;
  }

  // Active keys section
  let activeKeysHtml: string;
  if (activeKeys.length === 0) {
    activeKeysHtml = '<p class="empty">No active API keys.</p>';
  } else {
    const rows = activeKeys.map((k) => {
      const expires = k.expiresAt
        ? new Date(k.expiresAt).toISOString().slice(0, 16)
        : "never";
      return `
        <tr>
          <td>${escapeHtml(k.keyPrefix)}</td>
          <td>${escapeHtml(k.userEmail)}</td>
          <td>${escapeHtml(k.scopes.join(", "))}</td>
          <td>${new Date(k.createdAt).toISOString().slice(0, 10)}</td>
          <td>${expires}</td>
        </tr>
      `;
    }).join("");

    activeKeysHtml = `
      <table>
        <tr><th>Prefix</th><th>Email</th><th>Scopes</th><th>Created</th><th>Expires</th></tr>
        ${rows}
      </table>
    `;
  }

  // Expired keys section
  let expiredKeysHtml: string;
  if (expiredKeys.length === 0) {
    expiredKeysHtml = '<p class="empty">No recently expired keys.</p>';
  } else {
    const rows = expiredKeys.map((k) => `
      <tr>
        <td>${escapeHtml(k.keyPrefix)}</td>
        <td>${escapeHtml(k.userEmail)}</td>
        <td>${k.expiresAt ? new Date(k.expiresAt).toISOString().slice(0, 16) : "—"}</td>
      </tr>
    `).join("");

    expiredKeysHtml = `
      <table>
        <tr><th>Prefix</th><th>Email</th><th>Expired</th></tr>
        ${rows}
      </table>
    `;
  }

  return `<!DOCTYPE html>
<html>
<head>${styles}</head>
<body>
  <div class="container">
    <h1>SSMD Data Access Audit — ${dateStr}</h1>

    <h2>Downloads (last 24h) <span class="badge badge-active">${downloads.length}</span></h2>
    ${downloadsHtml}

    <h2>Active Keys <span class="badge badge-active">${activeKeys.length}</span></h2>
    ${activeKeysHtml}

    <h2>Recently Expired Keys (7 days) <span class="badge badge-expired">${expiredKeys.length}</span></h2>
    ${expiredKeysHtml}

    <div class="footer">
      Generated by ssmd audit-email at ${new Date().toISOString()}
    </div>
  </div>
</body>
</html>`;
}

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
