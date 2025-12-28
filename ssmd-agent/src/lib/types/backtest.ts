import { z } from "zod";

/**
 * Backtest manifest schema - defines what data to run a signal against.
 * Stored as backtest.yaml alongside signal.ts files.
 */
export const BacktestManifestSchema = z.object({
  /** Feed to pull data from (e.g., "kalshi") */
  feed: z.string(),
  /** Explicit list of dates to backtest */
  dates: z.array(z.string()).optional(),
  /** Date range to backtest (alternative to explicit dates) */
  date_range: z.object({
    from: z.string(),
    to: z.string(),
  }).optional(),
  /** Optional ticker filter */
  tickers: z.array(z.string()).optional(),
  /** Optional limit on records to process (for quick testing) */
  sample_limit: z.number().optional(),
}).refine(
  (data) => data.dates || data.date_range,
  { message: "Either dates or date_range must be provided" }
);

/**
 * Individual signal fire event
 */
export const SignalFireSchema = z.object({
  /** Timestamp when the signal fired */
  time: z.string(),
  /** Associated ticker (if applicable) */
  ticker: z.string().optional(),
  /** Signal-specific payload with context about the fire */
  payload: z.any(),
});

/**
 * Complete backtest result schema
 */
export const BacktestResultSchema = z.object({
  /** Unique run identifier */
  run_id: z.string(),
  /** Signal metadata at time of run */
  signal: z.object({
    id: z.string(),
    path: z.string(),
    git_sha: z.string(),
    dirty: z.boolean(),
  }),
  /** Data parameters used */
  data: z.object({
    feed: z.string(),
    dates: z.array(z.string()),
    records_processed: z.number(),
    tickers_seen: z.number(),
    data_fingerprint: z.string(),
  }),
  /** Backtest results */
  results: z.object({
    fire_count: z.number(),
    fire_rate: z.number(),
    fires: z.array(SignalFireSchema),
  }),
  /** Execution metadata */
  execution: z.object({
    started_at: z.string(),
    completed_at: z.string(),
    duration_ms: z.number(),
    worker_id: z.string(),
  }),
});

export type BacktestManifest = z.infer<typeof BacktestManifestSchema>;
export type BacktestResult = z.infer<typeof BacktestResultSchema>;
export type SignalFire = z.infer<typeof SignalFireSchema>;
