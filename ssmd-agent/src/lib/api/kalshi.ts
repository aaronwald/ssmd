/**
 * Kalshi API client with rate limiting and pagination
 */
import { RateLimiter, retry } from "../utils/rate-limit.ts";
import { fromKalshiEvent, type Event, type KalshiEvent } from "../types/event.ts";
import { fromKalshiMarket, type Market, type KalshiMarket } from "../types/market.ts";

/**
 * Kalshi API client configuration
 */
export interface KalshiClientOptions {
  /** API key for authentication */
  apiKey: string;
  /** Use demo environment (default: false) */
  demo?: boolean;
  /** Minimum delay between requests in ms (default: 250 = 4 req/sec) */
  minDelayMs?: number;
  /** Max retries for rate limiting (default: 10) */
  maxRetries?: number;
}

/**
 * Paginated response from Kalshi API
 */
interface PaginatedResponse<T> {
  cursor: string;
  [key: string]: T[] | string;
}

/**
 * Kalshi API client with rate limiting and pagination support
 */
export class KalshiClient {
  private readonly baseUrl: string;
  private readonly headers: Headers;
  private readonly limiter: RateLimiter;

  constructor(options: KalshiClientOptions) {
    this.baseUrl = options.demo
      ? "https://demo-api.kalshi.co/trade-api/v2"
      : "https://api.elections.kalshi.com/trade-api/v2";

    this.headers = new Headers({
      Authorization: `Bearer ${options.apiKey}`,
      "Content-Type": "application/json",
    });

    this.limiter = new RateLimiter(
      options.minDelayMs ?? 250,
      options.maxRetries ?? 10,
      5000 // Min retry wait
    );
  }

  /**
   * Make a rate-limited API request with retry support
   */
  private async fetch<T>(path: string): Promise<T> {
    await this.limiter.wait();

    const { maxRetries, minRetryWaitMs } = this.limiter.retryConfig;

    return retry(
      async () => {
        const res = await fetch(`${this.baseUrl}${path}`, {
          headers: this.headers,
        });
        this.limiter.markRequest();

        if (res.status === 429) {
          const retryAfter = res.headers.get("Retry-After");
          const wait = Math.max(minRetryWaitMs, parseInt(retryAfter || "5") * 1000);
          console.log(`  [API] 429 rate limited, waiting ${wait}ms`);
          throw new Error(`Rate limited, retry after ${wait}ms`);
        }

        if (!res.ok) {
          const text = await res.text();
          throw new Error(`API error ${res.status}: ${text}`);
        }

        return res.json();
      },
      {
        maxRetries,
        initialDelayMs: minRetryWaitMs,
        shouldRetry: (e) => e.message.includes("Rate limited"),
      }
    );
  }

  /**
   * Fetch all events with automatic pagination
   */
  async *fetchAllEvents(): AsyncGenerator<Event[]> {
    let cursor: string | undefined;
    let page = 0;

    do {
      const path = cursor
        ? `/events?cursor=${cursor}&limit=200`
        : "/events?limit=200";

      const data = await this.fetch<PaginatedResponse<KalshiEvent>>(path);
      const events = (data.events as KalshiEvent[]) || [];

      page++;
      console.log(`  [API] events page ${page}: ${events.length} fetched`);

      if (events.length > 0) {
        yield events.map(fromKalshiEvent);
      }

      cursor = data.cursor || undefined;
    } while (cursor);
  }

  /**
   * Fetch all markets with automatic pagination
   */
  async *fetchAllMarkets(): AsyncGenerator<Market[]> {
    let cursor: string | undefined;
    let page = 0;

    do {
      const path = cursor
        ? `/markets?cursor=${cursor}&limit=200`
        : "/markets?limit=200";

      const data = await this.fetch<PaginatedResponse<KalshiMarket>>(path);
      const markets = (data.markets as KalshiMarket[]) || [];

      page++;
      console.log(`  [API] markets page ${page}: ${markets.length} fetched`);

      if (markets.length > 0) {
        yield markets.map(fromKalshiMarket);
      }

      cursor = data.cursor || undefined;
    } while (cursor);
  }

  /**
   * Fetch a single event by ticker
   */
  async getEvent(eventTicker: string): Promise<Event | null> {
    try {
      const data = await this.fetch<{ event: KalshiEvent }>(
        `/events/${eventTicker}`
      );
      return fromKalshiEvent(data.event);
    } catch (e) {
      if ((e as Error).message.includes("404")) {
        return null;
      }
      throw e;
    }
  }

  /**
   * Fetch a single market by ticker
   */
  async getMarket(ticker: string): Promise<Market | null> {
    try {
      const data = await this.fetch<{ market: KalshiMarket }>(
        `/markets/${ticker}`
      );
      return fromKalshiMarket(data.market);
    } catch (e) {
      if ((e as Error).message.includes("404")) {
        return null;
      }
      throw e;
    }
  }
}

/**
 * Create a Kalshi client from environment variables
 */
export function createKalshiClient(): KalshiClient {
  const apiKey = Deno.env.get("KALSHI_API_KEY");
  if (!apiKey) {
    throw new Error("KALSHI_API_KEY environment variable not set");
  }

  const demo = Deno.env.get("KALSHI_DEMO") === "true";

  return new KalshiClient({ apiKey, demo });
}
