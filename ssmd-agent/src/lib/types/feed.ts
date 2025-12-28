// Feed types and Zod schemas - ported from internal/types/feed.go
import { z } from "zod";

// Enums
export const FeedTypeEnum = z.enum(["websocket", "rest", "multicast"]);
export const FeedStatusEnum = z.enum(["active", "deprecated", "disabled"]);
export const AuthMethodEnum = z.enum(["api_key", "oauth", "mtls", "none"]);
export const TransportProtocolEnum = z.enum(["wss", "https", "multicast", "tcp"]);
export const MessageProtocolEnum = z.enum(["json", "itch", "fix", "sbe", "protobuf"]);
export const SiteTypeEnum = z.enum(["cloud", "colo", "on_prem"]);

// Protocol schema
export const ProtocolSchema = z.object({
  transport: TransportProtocolEnum,
  message: MessageProtocolEnum,
  version: z.string().optional(),
});

// Capture location schema
export const CaptureLocationSchema = z.object({
  site: z.string(),
  type: SiteTypeEnum,
  provider: z.string().optional(),
  region: z.string().optional(),
  clock: z.string().optional(),
});

// Calendar schema
export const CalendarSchema = z.object({
  timezone: z.string().optional(),
  holiday_calendar: z.string().optional(),
  open_time: z.string().optional(),
  close_time: z.string().optional(),
});

// Feed version schema
export const FeedVersionSchema = z.object({
  version: z.string(),
  effective_from: z.string().regex(/^\d{4}-\d{2}-\d{2}$/, "Date must be YYYY-MM-DD"),
  effective_to: z.string().regex(/^\d{4}-\d{2}-\d{2}$/).optional(),
  protocol: ProtocolSchema,
  endpoint: z.string().url(),
  auth_method: AuthMethodEnum.optional(),
  rate_limit_per_second: z.number().int().positive().optional(),
  max_symbols_per_connection: z.number().int().positive().optional(),
  supports_orderbook: z.boolean().optional(),
  supports_trades: z.boolean().optional(),
  supports_historical: z.boolean().optional(),
  parser_config: z.record(z.string()).optional(),
});

// Main feed schema
export const FeedSchema = z.object({
  name: z.string().min(1, "Feed name is required"),
  display_name: z.string().optional(),
  type: FeedTypeEnum,
  status: FeedStatusEnum.default("active"),
  capture_locations: z.array(CaptureLocationSchema).optional(),
  versions: z.array(FeedVersionSchema).min(1, "Feed must have at least one version"),
  calendar: CalendarSchema.optional(),
});

// Type exports
export type FeedType = z.infer<typeof FeedTypeEnum>;
export type FeedStatus = z.infer<typeof FeedStatusEnum>;
export type AuthMethod = z.infer<typeof AuthMethodEnum>;
export type TransportProtocol = z.infer<typeof TransportProtocolEnum>;
export type MessageProtocol = z.infer<typeof MessageProtocolEnum>;
export type SiteType = z.infer<typeof SiteTypeEnum>;
export type Protocol = z.infer<typeof ProtocolSchema>;
export type CaptureLocation = z.infer<typeof CaptureLocationSchema>;
export type Calendar = z.infer<typeof CalendarSchema>;
export type FeedVersion = z.infer<typeof FeedVersionSchema>;
export type Feed = z.infer<typeof FeedSchema>;

// Helper functions

/**
 * Get the active version for a given date
 */
export function getVersionForDate(feed: Feed, date: Date): FeedVersion | null {
  const dateStr = date.toISOString().split("T")[0];
  const sorted = [...feed.versions].sort((a, b) =>
    b.effective_from.localeCompare(a.effective_from)
  );

  for (const v of sorted) {
    if (v.effective_from <= dateStr) {
      if (!v.effective_to || v.effective_to >= dateStr) {
        return v;
      }
    }
  }

  return null;
}

/**
 * Get the latest (most recent) version
 */
export function getLatestVersion(feed: Feed): FeedVersion | null {
  if (feed.versions.length === 0) return null;

  const sorted = [...feed.versions].sort((a, b) =>
    b.effective_from.localeCompare(a.effective_from)
  );

  return sorted[0];
}
