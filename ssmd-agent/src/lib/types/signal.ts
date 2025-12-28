import { z } from "zod";

/**
 * Signal definition schema for backtest evaluation.
 * Signals define conditions to watch for in market data.
 */
export const SignalSchema = z.object({
  /** Unique identifier for the signal */
  id: z.string().min(1),
  /** Human-readable name */
  name: z.string().optional(),
  /** State types required for evaluation (e.g., ["orderbook", "priceHistory"]) */
  requires: z.array(z.string()),
});

/**
 * Runtime signal with evaluate and payload functions.
 * This is what gets loaded from a signal.ts file.
 */
export interface Signal {
  id: string;
  name?: string;
  requires: string[];
  evaluate: (state: Record<string, unknown>) => boolean;
  payload: (state: Record<string, unknown>) => unknown;
}

export type SignalMetadata = z.infer<typeof SignalSchema>;
