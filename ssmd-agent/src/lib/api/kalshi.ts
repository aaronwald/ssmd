/**
 * Kalshi API client with rate limiting and pagination
 */
import { RateLimiter, retry } from "../utils/rate-limit.ts";
import { fromKalshiEvent, type Event, type KalshiEvent } from "../types/event.ts";
import { fromKalshiMarket, type Market, type KalshiMarket } from "../types/market.ts";
import {
  fromKalshiFeeChange,
  type SeriesFeeChange,
  type KalshiFeeChange,
} from "../types/fee.ts";

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
 * Market query filters
 *
 * Note on filter compatibility (per Kalshi API docs):
 * - min_created_ts, max_created_ts: compatible with status=unopened, open, or empty
 * - min_close_ts, max_close_ts: compatible with status=closed or empty
 * - min_settled_ts, max_settled_ts: compatible with status=settled or empty
 */
export interface MarketFilters {
  /** Status filter (e.g., 'open', 'closed', 'settled') */
  status?: string;
  /** Minimum created timestamp (Unix seconds) - for recently created markets */
  minCreatedTs?: number;
  /** Minimum close timestamp (Unix seconds) - for recently closed markets */
  minCloseTs?: number;
  /** Maximum close timestamp (Unix seconds) */
  maxCloseTs?: number;
  /** Minimum settled timestamp (Unix seconds) - for recently settled markets */
  minSettledTs?: number;
  /** MVE filter: 'exclude' to exclude multiverse markets */
  mveFilter?: "exclude" | "only";
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

    // Conservative rate: 1 req/sec to avoid 429s during bulk sync
    // Kalshi's limit is ~10 req/sec but bulk syncs hit it easily
    this.limiter = new RateLimiter(
      options.minDelayMs ?? 1000,
      options.maxRetries ?? 10,
      10000 // Min retry wait (10s) with exponential backoff
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
   * @param status - Optional status filter (e.g., 'open' for active only)
   */
  async *fetchAllEvents(status?: string): AsyncGenerator<Event[]> {
    let cursor: string | undefined;
    let page = 0;
    const statusParam = status ? `&status=${status}` : "";

    do {
      const path = cursor
        ? `/events?cursor=${cursor}&limit=200${statusParam}`
        : `/events?limit=200${statusParam}`;

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
   * @param filters - Optional filters (status, minCloseTs, etc.)
   */
  async *fetchAllMarkets(filters?: MarketFilters): AsyncGenerator<Market[]> {
    let cursor: string | undefined;
    let page = 0;

    // Build query params from filters
    const params: string[] = [];
    if (filters?.status) params.push(`status=${filters.status}`);
    if (filters?.minCreatedTs) params.push(`min_created_ts=${filters.minCreatedTs}`);
    if (filters?.minCloseTs) params.push(`min_close_ts=${filters.minCloseTs}`);
    if (filters?.maxCloseTs) params.push(`max_close_ts=${filters.maxCloseTs}`);
    if (filters?.minSettledTs) params.push(`min_settled_ts=${filters.minSettledTs}`);
    if (filters?.mveFilter) params.push(`mve_filter=${filters.mveFilter}`);
    const filterParams = params.length > 0 ? "&" + params.join("&") : "";

    do {
      const path = cursor
        ? `/markets?cursor=${cursor}&limit=200${filterParams}`
        : `/markets?limit=200${filterParams}`;

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

  /**
   * Fetch all fee changes (including historical).
   * No pagination needed - returns all at once.
   */
  async fetchFeeChanges(showHistorical = true): Promise<SeriesFeeChange[]> {
    const path = `/series/fee_changes?show_historical=${showHistorical}`;

    console.log(`  [API] Fetching fee changes (historical: ${showHistorical})`);

    const data = await this.fetch<{ series_fee_change_arr: KalshiFeeChange[] }>(
      path
    );

    const changes = data.series_fee_change_arr || [];
    console.log(`  [API] Fetched ${changes.length} fee changes`);

    return changes.map(fromKalshiFeeChange);
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
