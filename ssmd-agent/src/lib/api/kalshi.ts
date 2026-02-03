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
 * Series metadata from Kalshi API
 */
export interface KalshiSeries {
  ticker: string;
  title: string;
  category: string;
  tags?: string[];
  volume?: number;
}

/**
 * Tags grouped by category from /search/tags_by_categories
 */
export type TagsByCategories = Record<string, string[]>;

/**
 * Sport filter structure from /search/filters_by_sport
 */
export interface SportFilters {
  sports: Array<{
    sport: string;
    competitions: Array<{
      competition: string;
      scopes: string[];
    }>;
  }>;
}

/**
 * Kalshi API client configuration
 */
export interface KalshiClientOptions {
  /** Use demo environment (default: false) */
  demo?: boolean;
  /** Minimum delay between requests in ms (default: 1000 = 1 req/sec) */
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

  constructor(options: KalshiClientOptions = {}) {
    this.baseUrl = options.demo
      ? "https://demo-api.kalshi.co/trade-api/v2"
      : "https://api.elections.kalshi.com/trade-api/v2";

    this.headers = new Headers({
      "Content-Type": "application/json",
    });

    // Rate limit: 300ms between requests (~3 req/sec)
    // Plus 1 second delay between series in secmaster sync
    this.limiter = new RateLimiter(
      options.minDelayMs ?? 300,
      options.maxRetries ?? 10,
      5000 // Min retry wait (5s) with exponential backoff
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
        const url = `${this.baseUrl}${path}`;

        const res = await fetch(url, {
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

      if (events.length > 0) {
        yield events.map(fromKalshiEvent);
      }

      cursor = data.cursor || undefined;
    } while (cursor);
  }

  /**
   * Fetch events by series with nested markets.
   * Uses series_ticker filter and with_nested_markets=true.
   * @param seriesTicker - Series ticker to filter by
   * @param status - Optional status filter (e.g., 'open')
   */
  async *fetchEventsBySeries(
    seriesTicker: string,
    status?: string
  ): AsyncGenerator<{ events: Event[]; markets: Market[] }> {
    let cursor: string | undefined;
    let page = 0;

    const params: string[] = [
      `series_ticker=${encodeURIComponent(seriesTicker)}`,
      "with_nested_markets=true",
    ];
    if (status) params.push(`status=${status}`);
    const queryParams = params.join("&");

    do {
      const path = cursor
        ? `/events?cursor=${cursor}&limit=200&${queryParams}`
        : `/events?limit=200&${queryParams}`;

      const data = await this.fetch<PaginatedResponse<KalshiEvent>>(path);
      const rawEvents = (data.events as KalshiEvent[]) || [];

      page++;

      if (rawEvents.length > 0) {
        const events = rawEvents.map(fromKalshiEvent);
        const markets: Market[] = [];
        for (const e of rawEvents) {
          if (e.markets) {
            markets.push(...e.markets.map(fromKalshiMarket));
          }
        }
        yield { events, markets };
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
    const data = await this.fetch<{ series_fee_change_arr: KalshiFeeChange[] }>(path);
    const changes = data.series_fee_change_arr || [];
    return changes.map(fromKalshiFeeChange);
  }

  /**
   * Fetch tags grouped by category from /search/tags_by_categories
   */
  async fetchTagsByCategories(): Promise<TagsByCategories> {
    return this.fetch<TagsByCategories>(`/search/tags_by_categories`);
  }

  /**
   * Fetch sport filters from /search/filters_by_sport
   */
  async fetchFiltersBySport(): Promise<SportFilters> {
    return this.fetch<SportFilters>(`/search/filters_by_sport`);
  }

  /**
   * Fetch series with optional category and tag filters
   * @param category - Filter by category (e.g., 'Sports', 'Economics')
   * @param tag - Filter by tag within category (e.g., 'Basketball')
   * @param includeVolume - Include volume data (for filtering by volume)
   */
  async *fetchSeries(category?: string, tag?: string, includeVolume?: boolean): AsyncGenerator<KalshiSeries[]> {
    let cursor: string | undefined;
    let page = 0;

    const params: string[] = [];
    if (category) params.push(`category=${encodeURIComponent(category)}`);
    if (tag) params.push(`tags=${encodeURIComponent(tag)}`);
    if (includeVolume) params.push("include_volume=true");
    const queryParams = params.length > 0 ? params.join("&") : "";

    do {
      const path = cursor
        ? `/series?cursor=${cursor}&${queryParams}`
        : `/series${queryParams ? "?" + queryParams : ""}`;

      const data = await this.fetch<PaginatedResponse<KalshiSeries>>(path);
      const series = (data.series as KalshiSeries[]) || [];

      page++;

      if (series.length > 0) {
        yield series;
      }

      cursor = data.cursor || undefined;
    } while (cursor);
  }

  /**
   * Fetch all series for a category (convenience method)
   */
  async fetchAllSeries(category?: string, tag?: string, includeVolume?: boolean): Promise<KalshiSeries[]> {
    const allSeries: KalshiSeries[] = [];
    for await (const batch of this.fetchSeries(category, tag, includeVolume)) {
      allSeries.push(...batch);
    }
    return allSeries;
  }

  /**
   * Fetch markets by series ticker with status filter
   * @param seriesTicker - Series ticker (e.g., 'KXNBAGAME')
   * @param filters - Optional filters (status, timestamps)
   */
  async *fetchMarketsBySeries(
    seriesTicker: string,
    filters?: MarketFilters
  ): AsyncGenerator<Market[]> {
    let cursor: string | undefined;
    let page = 0;

    // Build query params
    const params: string[] = [`series_ticker=${encodeURIComponent(seriesTicker)}`];
    if (filters?.status) params.push(`status=${filters.status}`);
    if (filters?.minCloseTs) params.push(`min_close_ts=${filters.minCloseTs}`);
    if (filters?.maxCloseTs) params.push(`max_close_ts=${filters.maxCloseTs}`);
    if (filters?.minSettledTs) params.push(`min_settled_ts=${filters.minSettledTs}`);
    const filterParams = params.join("&");

    do {
      const path = cursor
        ? `/markets?cursor=${cursor}&${filterParams}`
        : `/markets?${filterParams}`;

      const data = await this.fetch<PaginatedResponse<KalshiMarket>>(path);
      const markets = (data.markets as KalshiMarket[]) || [];

      page++;

      if (markets.length > 0) {
        yield markets.map(fromKalshiMarket);
      }

      cursor = data.cursor || undefined;
    } while (cursor);
  }

  /**
   * Fetch all markets for a series (convenience method)
   */
  async fetchAllMarketsBySeries(
    seriesTicker: string,
    filters?: MarketFilters
  ): Promise<Market[]> {
    const allMarkets: Market[] = [];
    for await (const batch of this.fetchMarketsBySeries(seriesTicker, filters)) {
      allMarkets.push(...batch);
    }
    return allMarkets;
  }

  /**
   * Fetch trades for a specific market ticker with time range filtering
   * @param ticker - Market ticker (e.g., 'KXBTCD-26FEB0317-T76999.99')
   * @param minTs - Minimum timestamp (Unix seconds)
   * @param maxTs - Maximum timestamp (Unix seconds)
   * @param limit - Max trades per page (default 1000)
   */
  async *fetchTrades(
    ticker: string,
    minTs?: number,
    maxTs?: number,
    limit = 1000
  ): AsyncGenerator<KalshiTrade[]> {
    let cursor: string | undefined;

    const params: string[] = [`ticker=${encodeURIComponent(ticker)}`, `limit=${limit}`];
    if (minTs) params.push(`min_ts=${minTs}`);
    if (maxTs) params.push(`max_ts=${maxTs}`);
    const queryParams = params.join("&");

    do {
      const path = cursor
        ? `/markets/trades?cursor=${cursor}&${queryParams}`
        : `/markets/trades?${queryParams}`;

      const data = await this.fetch<{ cursor: string; trades: KalshiTrade[] }>(path);
      const trades = data.trades || [];

      if (trades.length > 0) {
        yield trades;
      }

      cursor = data.cursor || undefined;
    } while (cursor);
  }

  /**
   * Fetch all trades for a ticker (convenience method)
   */
  async fetchAllTrades(
    ticker: string,
    minTs?: number,
    maxTs?: number
  ): Promise<KalshiTrade[]> {
    const allTrades: KalshiTrade[] = [];
    for await (const batch of this.fetchTrades(ticker, minTs, maxTs)) {
      allTrades.push(...batch);
    }
    return allTrades;
  }
}

/**
 * Trade from Kalshi API
 */
export interface KalshiTrade {
  trade_id: string;
  ticker: string;
  yes_price: number;
  no_price: number;
  count: number;
  taker_side: string;
  created_time: string;
}

/**
 * Create a Kalshi client (no auth needed for public read-only endpoints)
 */
export function createKalshiClient(): KalshiClient {
  const demo = Deno.env.get("KALSHI_DEMO") === "true";
  return new KalshiClient({ demo });
}
