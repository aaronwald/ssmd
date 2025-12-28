import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { MetricsRegistry, Counter, Histogram } from "../../src/server/metrics.ts";

Deno.test("Counter increments correctly", () => {
  const registry = new MetricsRegistry();
  const counter = registry.counter("test_counter", "Test counter");

  counter.inc();
  counter.inc();
  counter.inc();

  const output = registry.format();
  assertEquals(output.includes("test_counter 3"), true);
});

Deno.test("Counter increments with labels", () => {
  const registry = new MetricsRegistry();
  const counter = registry.counter("http_requests_total", "Total requests", ["method"]);

  counter.inc({ method: "GET" });
  counter.inc({ method: "GET" });
  counter.inc({ method: "POST" });

  const output = registry.format();
  assertEquals(output.includes('http_requests_total{method="GET"} 2'), true);
  assertEquals(output.includes('http_requests_total{method="POST"} 1'), true);
});

Deno.test("Counter includes HELP and TYPE", () => {
  const registry = new MetricsRegistry();
  registry.counter("my_counter", "A helpful description");

  const output = registry.format();
  assertEquals(output.includes("# HELP my_counter A helpful description"), true);
  assertEquals(output.includes("# TYPE my_counter counter"), true);
});

Deno.test("Histogram observes values correctly", () => {
  const registry = new MetricsRegistry();
  const hist = registry.histogram(
    "request_duration_seconds",
    "Request duration",
    [],
    [0.1, 0.5, 1]
  );

  hist.observe({}, 0.05);
  hist.observe({}, 0.3);
  hist.observe({}, 0.8);

  const output = registry.format();
  assertEquals(output.includes("request_duration_seconds_bucket"), true);
  assertEquals(output.includes("request_duration_seconds_sum"), true);
  assertEquals(output.includes("request_duration_seconds_count 3"), true);
});

Deno.test("Histogram buckets count correctly", () => {
  const registry = new MetricsRegistry();
  const hist = registry.histogram(
    "latency",
    "Latency",
    [],
    [0.1, 0.5, 1]
  );

  hist.observe({}, 0.05); // <= 0.1, 0.5, 1, +Inf
  hist.observe({}, 0.3);  // <= 0.5, 1, +Inf
  hist.observe({}, 0.8);  // <= 1, +Inf
  hist.observe({}, 2.0);  // <= +Inf only

  const output = registry.format();
  assertEquals(output.includes('latency_bucket{le="0.1"} 1'), true);
  assertEquals(output.includes('latency_bucket{le="0.5"} 2'), true);
  assertEquals(output.includes('latency_bucket{le="1"} 3'), true);
  assertEquals(output.includes('latency_bucket{le="+Inf"} 4'), true);
});

Deno.test("Histogram with labels", () => {
  const registry = new MetricsRegistry();
  const hist = registry.histogram(
    "http_request_duration",
    "HTTP request duration",
    ["method", "path"],
    [0.1, 0.5]
  );

  hist.observe({ method: "GET", path: "/health" }, 0.05);
  hist.observe({ method: "GET", path: "/health" }, 0.03);
  hist.observe({ method: "POST", path: "/data" }, 0.2);

  const output = registry.format();
  assertEquals(output.includes('method="GET"'), true);
  assertEquals(output.includes('path="/health"'), true);
  assertEquals(output.includes('method="POST"'), true);
});

Deno.test("Registry formats multiple metrics", () => {
  const registry = new MetricsRegistry();
  registry.counter("requests_total", "Total requests");
  registry.counter("errors_total", "Total errors");

  const output = registry.format();
  assertEquals(output.includes("# HELP requests_total"), true);
  assertEquals(output.includes("# HELP errors_total"), true);
});
