/**
 * Rate limiter for API requests
 */
export class RateLimiter {
  private lastRequest = 0;

  constructor(
    private readonly minDelayMs: number,
    private readonly maxRetries: number = 10,
    private readonly minRetryWaitMs: number = 5000
  ) {}

  /**
   * Wait until enough time has passed since the last request.
   */
  async wait(): Promise<void> {
    const elapsed = Date.now() - this.lastRequest;
    if (elapsed < this.minDelayMs) {
      await new Promise((r) => setTimeout(r, this.minDelayMs - elapsed));
    }
  }

  /**
   * Mark that a request was just made.
   */
  markRequest(): void {
    this.lastRequest = Date.now();
  }

  /**
   * Get retry configuration.
   */
  get retryConfig() {
    return {
      maxRetries: this.maxRetries,
      minRetryWaitMs: this.minRetryWaitMs,
    };
  }
}

/**
 * Sleep for a specified number of milliseconds.
 */
export function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

/**
 * Retry a function with exponential backoff.
 */
export async function retry<T>(
  fn: () => Promise<T>,
  options: {
    maxRetries?: number;
    initialDelayMs?: number;
    maxDelayMs?: number;
    shouldRetry?: (error: Error) => boolean;
  } = {}
): Promise<T> {
  const {
    maxRetries = 3,
    initialDelayMs = 1000,
    maxDelayMs = 30000,
    shouldRetry = () => true,
  } = options;

  let lastError: Error | undefined;
  let delay = initialDelayMs;

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      return await fn();
    } catch (e) {
      lastError = e as Error;

      if (attempt === maxRetries || !shouldRetry(lastError)) {
        throw lastError;
      }

      console.log(`  Retry ${attempt + 1}/${maxRetries} after ${delay}ms: ${lastError.message}`);
      await sleep(delay);
      delay = Math.min(delay * 2, maxDelayMs);
    }
  }

  throw lastError;
}
