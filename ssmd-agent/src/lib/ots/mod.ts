export interface OtsOptions {
  username?: string;
  apiToken?: string;
  ttlSeconds: number;
  passphrase?: string;
  recipient?: string;
  baseUrl?: string; // default https://onetimesecret.com
  timeoutMs?: number; // default 10_000
}

/** Store a secret on onetimesecret.com; return the public one-time link.
 * Uses HTTP Basic auth when both username and apiToken are set; otherwise
 * posts anonymously. Throws on non-2xx or a missing secret_key. */
export async function createOneTimeSecret(secret: string, opts: OtsOptions): Promise<string> {
  const base = opts.baseUrl ?? "https://onetimesecret.com";
  const body = new URLSearchParams({ secret, ttl: String(opts.ttlSeconds) });
  if (opts.passphrase) body.set("passphrase", opts.passphrase);
  if (opts.recipient) body.set("recipient", opts.recipient);

  const headers: Record<string, string> = { "Content-Type": "application/x-www-form-urlencoded" };
  if (opts.username && opts.apiToken) {
    headers["Authorization"] = `Basic ${btoa(`${opts.username}:${opts.apiToken}`)}`;
  }

  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), opts.timeoutMs ?? 10_000);
  try {
    const res = await fetch(`${base}/api/v1/share`, { method: "POST", headers, body, signal: ctrl.signal });
    if (!res.ok) throw new Error(`onetimesecret share failed: ${res.status}`);
    const data = await res.json() as { secret_key?: string };
    if (!data.secret_key) throw new Error("onetimesecret response missing secret_key");
    return `${base}/secret/${data.secret_key}`;
  } finally {
    clearTimeout(timer);
  }
}
