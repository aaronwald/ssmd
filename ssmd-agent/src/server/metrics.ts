/**
 * Simple Prometheus metrics implementation for ssmd-data-ts
 */

type Labels = Record<string, string>;

interface Metric {
  format(): string;
}

/**
 * Format labels into Prometheus format: {key="value",key2="value2"}
 */
function formatLabels(labels: Labels): string {
  const parts = Object.entries(labels).map(([k, v]) => `${k}="${v}"`);
  return parts.length > 0 ? `{${parts.join(",")}}` : "";
}

/**
 * Prometheus Counter metric
 */
export class Counter implements Metric {
  private values: Map<string, number> = new Map();

  constructor(
    private name: string,
    private help: string,
    private labelNames: string[] = []
  ) {}

  /**
   * Increment the counter
   */
  inc(labels: Labels = {}, value = 1): void {
    const key = JSON.stringify(labels);
    this.values.set(key, (this.values.get(key) ?? 0) + value);
  }

  format(): string {
    const lines: string[] = [];
    lines.push(`# HELP ${this.name} ${this.help}`);
    lines.push(`# TYPE ${this.name} counter`);

    if (this.values.size === 0) {
      // Output zero value for counter with no labels
      if (this.labelNames.length === 0) {
        lines.push(`${this.name} 0`);
      }
    } else {
      for (const [key, value] of this.values) {
        const labels = JSON.parse(key) as Labels;
        lines.push(`${this.name}${formatLabels(labels)} ${value}`);
      }
    }

    return lines.join("\n");
  }
}

/**
 * Prometheus Histogram metric
 */
export class Histogram implements Metric {
  private bucketCounts: Map<string, Map<number, number>> = new Map();
  private sums: Map<string, number> = new Map();
  private counts: Map<string, number> = new Map();
  private sortedBuckets: number[];

  constructor(
    private name: string,
    private help: string,
    private labelNames: string[] = [],
    buckets: number[]
  ) {
    // Ensure buckets are sorted and include +Inf
    this.sortedBuckets = [...buckets].sort((a, b) => a - b);
    if (!this.sortedBuckets.includes(Infinity)) {
      this.sortedBuckets.push(Infinity);
    }
  }

  /**
   * Record an observation
   */
  observe(labels: Labels = {}, value: number): void {
    const key = JSON.stringify(labels);

    // Initialize if needed
    if (!this.bucketCounts.has(key)) {
      const bucketMap = new Map<number, number>();
      for (const b of this.sortedBuckets) {
        bucketMap.set(b, 0);
      }
      this.bucketCounts.set(key, bucketMap);
      this.sums.set(key, 0);
      this.counts.set(key, 0);
    }

    // Update buckets (cumulative)
    const bucketMap = this.bucketCounts.get(key)!;
    for (const b of this.sortedBuckets) {
      if (value <= b) {
        bucketMap.set(b, bucketMap.get(b)! + 1);
      }
    }

    this.sums.set(key, this.sums.get(key)! + value);
    this.counts.set(key, this.counts.get(key)! + 1);
  }

  format(): string {
    const lines: string[] = [];
    lines.push(`# HELP ${this.name} ${this.help}`);
    lines.push(`# TYPE ${this.name} histogram`);

    for (const [key, bucketMap] of this.bucketCounts) {
      const labels = JSON.parse(key) as Labels;

      // Output bucket lines
      for (const [le, count] of bucketMap) {
        const bucketLabels = { ...labels, le: le === Infinity ? "+Inf" : String(le) };
        lines.push(`${this.name}_bucket${formatLabels(bucketLabels)} ${count}`);
      }

      // Output sum and count
      lines.push(`${this.name}_sum${formatLabels(labels)} ${this.sums.get(key)}`);
      lines.push(`${this.name}_count${formatLabels(labels)} ${this.counts.get(key)}`);
    }

    return lines.join("\n");
  }
}

/**
 * Metrics Registry - holds all registered metrics
 */
export class MetricsRegistry {
  private metrics: Map<string, Metric> = new Map();

  /**
   * Create and register a counter
   */
  counter(name: string, help: string, labels: string[] = []): Counter {
    const counter = new Counter(name, help, labels);
    this.metrics.set(name, counter);
    return counter;
  }

  /**
   * Create and register a histogram
   */
  histogram(
    name: string,
    help: string,
    labels: string[],
    buckets: number[]
  ): Histogram {
    const hist = new Histogram(name, help, labels, buckets);
    this.metrics.set(name, hist);
    return hist;
  }

  /**
   * Format all metrics for Prometheus scraping
   */
  format(): string {
    const lines: string[] = [];
    for (const metric of this.metrics.values()) {
      lines.push(metric.format());
    }
    return lines.join("\n\n");
  }
}

// Global registry for the server
export const globalRegistry = new MetricsRegistry();

// Pre-defined metrics for ssmd-data-ts
export const httpRequestDuration = globalRegistry.histogram(
  "ssmd_data_http_request_duration_seconds",
  "HTTP request latency in seconds",
  ["method", "path", "status"],
  [0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5]
);

export const httpRequestsTotal = globalRegistry.counter(
  "ssmd_data_http_requests_total",
  "Total HTTP requests",
  ["method", "path", "status"]
);

export const recordsServed = globalRegistry.counter(
  "ssmd_data_records_served_total",
  "Total records served from datasets",
  ["feed"]
);

export const datasetsScanned = globalRegistry.counter(
  "ssmd_data_datasets_scanned_total",
  "Total datasets scanned",
  ["feed"]
);
