import { z } from "zod";

/**
 * Fee type enum matching Kalshi API
 */
export const FeeTypeSchema = z.enum([
  "quadratic",
  "quadratic_with_maker_fees",
  "flat",
]);
export type FeeType = z.infer<typeof FeeTypeSchema>;

/**
 * Fee change from Kalshi API
 */
export const SeriesFeeChangeSchema = z.object({
  /** Unique ID for this fee change record */
  id: z.string(),
  /** Series ticker this fee applies to */
  series_ticker: z.string(),
  /** Fee calculation type */
  fee_type: FeeTypeSchema,
  /** Series-specific multiplier (default 1.0) */
  fee_multiplier: z.number(),
  /** When this fee schedule becomes effective */
  scheduled_ts: z.string().datetime(),
});
export type SeriesFeeChange = z.infer<typeof SeriesFeeChangeSchema>;

/**
 * Series fee schedule from database
 */
export interface SeriesFee {
  id: number;
  series_ticker: string;
  fee_type: FeeType;
  fee_multiplier: number;
  effective_from: Date;
  effective_to: Date | null;
  source_id: string | null;
  created_at: Date;
}

/**
 * Kalshi API fee change response shape
 */
export interface KalshiFeeChange {
  id: string;
  series_ticker: string;
  fee_type: string;
  fee_multiplier: number;
  scheduled_ts: string;
}

/**
 * Convert Kalshi API fee change to our SeriesFeeChange type
 */
export function fromKalshiFeeChange(kfc: KalshiFeeChange): SeriesFeeChange {
  // Validate and parse the fee type
  const feeType = FeeTypeSchema.safeParse(kfc.fee_type);
  if (!feeType.success) {
    throw new Error(`Unknown fee type: ${kfc.fee_type}`);
  }

  return {
    id: kfc.id,
    series_ticker: kfc.series_ticker,
    fee_type: feeType.data,
    fee_multiplier: kfc.fee_multiplier,
    scheduled_ts: kfc.scheduled_ts,
  };
}
