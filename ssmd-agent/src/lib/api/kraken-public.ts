/**
 * Minimal Kraken public REST client for trade fetching (DQ reconciliation)
 */

const KRAKEN_TRADES_URL = "https://api.kraken.com/0/public/Trades";

export interface KrakenPublicTrade {
  price: string;
  volume: string;
  time: number;
  side: string; // "buy" or "sell"
  type: string; // "market" or "limit"
  tradeId: string;
}

/**
 * Fetch recent trades for a Kraken pair.
 * The `since` parameter is a nonce (nanosecond timestamp), not a Unix timestamp.
 * Returns trades and the `last` nonce for pagination.
 */
export async function fetchKrakenTrades(
  pair: string,
  since?: string,
): Promise<{ trades: KrakenPublicTrade[]; last: string }> {
  const params = new URLSearchParams({ pair });
  if (since) params.set("since", since);

  const res = await fetch(`${KRAKEN_TRADES_URL}?${params}`, {
    signal: AbortSignal.timeout(30000),
  });
  if (!res.ok) {
    throw new Error(`Kraken trades API error: ${res.status}`);
  }
  const data = await res.json();
  if (data.error?.length > 0) {
    throw new Error(`Kraken API errors: ${data.error.join(", ")}`);
  }

  // Response format: { result: { "XXBTZUSD": [[price, volume, time, side, type, tradeId, ...], ...], last: "nonce" } }
  const resultKeys = Object.keys(data.result).filter((k) => k !== "last");
  const rawTrades = data.result[resultKeys[0]] ?? [];
  const last = String(data.result.last ?? "");

  const trades: KrakenPublicTrade[] = rawTrades.map(
    (t: (string | number)[]) => ({
      price: String(t[0]),
      volume: String(t[1]),
      time: Number(t[2]),
      side: t[3] === "b" ? "buy" : "sell",
      type: t[4] === "m" ? "market" : "limit",
      tradeId: String(t[5] ?? `${t[2]}-${t[0]}-${t[1]}`),
    }),
  );

  return { trades, last };
}
