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
 * Add jitter to a delay value (±25%)
 */
function addJitter(delayMs: number): number {
  const jitter = delayMs * 0.25 * (Math.random() * 2 - 1);
  return Math.round(delayMs + jitter);
}

/**
 * Retry a function with exponential backoff and jitter.
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
    maxDelayMs = 60000,
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

      const jitteredDelay = addJitter(delay);
      console.log(`  Retry ${attempt + 1}/${maxRetries} after ${jitteredDelay}ms: ${lastError.message}`);
      await sleep(jitteredDelay);
      delay = Math.min(delay * 2, maxDelayMs);
    }
  }

  throw lastError;
}
