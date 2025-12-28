import { assertEquals, assertRejects } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { RateLimiter, sleep, retry } from "../../../src/lib/utils/rate-limit.ts";

Deno.test("RateLimiter enforces minimum delay", async () => {
  const limiter = new RateLimiter(100); // 100ms minimum

  limiter.markRequest();
  const start = Date.now();
  await limiter.wait();
  const elapsed = Date.now() - start;

  // Should have waited close to 100ms
  assertEquals(elapsed >= 90, true, `Expected >= 90ms, got ${elapsed}ms`);
});

Deno.test("RateLimiter does not wait if enough time passed", async () => {
  const limiter = new RateLimiter(50);

  // Don't mark any request, so no wait needed
  const start = Date.now();
  await limiter.wait();
  const elapsed = Date.now() - start;

  assertEquals(elapsed < 10, true, `Expected < 10ms, got ${elapsed}ms`);
});

Deno.test("RateLimiter exposes retry config", () => {
  const limiter = new RateLimiter(100, 5, 2000);

  assertEquals(limiter.retryConfig.maxRetries, 5);
  assertEquals(limiter.retryConfig.minRetryWaitMs, 2000);
});

Deno.test("sleep waits for specified time", async () => {
  const start = Date.now();
  await sleep(50);
  const elapsed = Date.now() - start;

  assertEquals(elapsed >= 45, true, `Expected >= 45ms, got ${elapsed}ms`);
});

Deno.test("retry succeeds on first attempt", async () => {
  let attempts = 0;
  const result = await retry(async () => {
    attempts++;
    return "success";
  });

  assertEquals(result, "success");
  assertEquals(attempts, 1);
});

Deno.test("retry retries on failure", async () => {
  let attempts = 0;
  const result = await retry(
    async () => {
      attempts++;
      if (attempts < 3) {
        throw new Error("fail");
      }
      return "success";
    },
    { maxRetries: 3, initialDelayMs: 10 }
  );

  assertEquals(result, "success");
  assertEquals(attempts, 3);
});

Deno.test("retry throws after max retries", async () => {
  let attempts = 0;
  await assertRejects(
    async () => {
      await retry(
        async () => {
          attempts++;
          throw new Error("always fails");
        },
        { maxRetries: 2, initialDelayMs: 10 }
      );
    },
    Error,
    "always fails"
  );

  assertEquals(attempts, 3); // Initial + 2 retries
});

Deno.test("retry respects shouldRetry predicate", async () => {
  let attempts = 0;
  await assertRejects(
    async () => {
      await retry(
        async () => {
          attempts++;
          throw new Error("not retryable");
        },
        {
          maxRetries: 5,
          initialDelayMs: 10,
          shouldRetry: (e) => !e.message.includes("not retryable"),
        }
      );
    },
    Error,
    "not retryable"
  );

  assertEquals(attempts, 1); // Should not retry
});
