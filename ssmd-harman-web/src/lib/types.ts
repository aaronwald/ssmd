export type Side = "yes" | "no";
export type Action = "buy" | "sell";
export type TimeInForce = "gtc" | "ioc";

export type OrderType = "limit" | "market";

export type OrderState =
  | "pending"
  | "submitted"
  | "acknowledged"
  | "partially_filled"
  | "filled"
  | "staged"
  | "monitoring"
  | "pending_cancel"
  | "pending_amend"
  | "pending_decrease"
  | "cancelled"
  | "rejected"
  | "expired";

export type LegRole = "entry" | "take_profit" | "stop_loss" | "oco_leg" | null;
export type GroupType = "bracket" | "oco";
export type GroupState = "pending" | "active" | "completed" | "cancelled";

export interface Order {
  id: number;
  client_order_id: string;
  exchange_order_id: string | null;
  ticker: string;
  side: Side;
  action: Action;
  quantity: string;
  price_dollars: string;
  filled_quantity: string;
  time_in_force: TimeInForce;
  state: OrderState;
  cancel_reason: string | null;
  order_type: OrderType;
  trigger_price: string | null;
  group_id: number | null;
  leg_role: LegRole;
  created_at: string;
  updated_at: string;
}

export interface OrderGroup {
  id: number;
  group_type: GroupType;
  state: GroupState;
  orders: Order[];
  created_at: string;
  updated_at: string;
}

export interface Fill {
  id: number;
  order_id: number;
  ticker: string;
  side: Side;
  action: Action;
  price: string;
  quantity: string;
  is_taker: boolean;
  filled_at: string;
}

export interface AuditEntry {
  id: number;
  order_id: number | null;
  group_id: number | null;
  event_type: string;
  detail: string;
  created_at: string;
}

export interface ExchangePosition {
  ticker: string;
  side: Side;
  quantity: string;
  market_value_dollars: string;
}

export interface LocalPosition {
  ticker: string;
  net_quantity: string;
  buy_filled: string;
  sell_filled: string;
}

export interface PositionsView {
  exchange: ExchangePosition[];
  local: LocalPosition[];
}

export interface RiskResponse {
  open_notional: string;
  max_notional: string;
  available_notional: string;
}

export interface CreateOrderRequest {
  client_order_id: string;
  ticker: string;
  side: Side;
  action: Action;
  quantity: string;
  price_dollars: string;
  time_in_force: TimeInForce;
  order_type?: OrderType;
  trigger_price?: string;
}

export interface CreateBracketRequest {
  entry: CreateOrderRequest;
  take_profit: CreateOrderRequest;
  stop_loss: CreateOrderRequest;
}

export interface CreateOcoRequest {
  leg1: CreateOrderRequest;
  leg2: CreateOrderRequest;
}

export interface HealthResponse {
  status: string;
  session_state: string;
  uptime_seconds: number;
}

export interface SnapResponse {
  feed: string;
  snapshots: RawKalshiSnapshot[];
  count: number;
}

/** Raw Kalshi snapshot from snap endpoint — prices in msg as cents + dollar strings */
export interface RawKalshiSnapshot {
  _ticker: string;
  msg?: {
    yes_bid_dollars?: string;
    yes_ask_dollars?: string;
    price_dollars?: string;
    yes_bid?: number;
    yes_ask?: number;
    price?: number;
    volume?: number;
    open_interest?: number;
  };
}

/** Normalized snapshot for display — all prices in dollars */
export interface NormalizedSnapshot {
  ticker: string;
  yesBid: number | null;
  yesAsk: number | null;
  last: number | null;
  /** Epoch millis when snap was written to Redis (injected by snap service) */
  snapAt: number | null;
}

/** Monitor hierarchy types */
export interface MonitorCategory {
  name: string;
  // Kalshi
  event_count?: number;
  series_count?: number;
  // Kraken
  base_count?: number;
  instrument_count?: number;
  // Polymarket
  pm_condition_count?: number;
}

export interface MonitorSeries {
  ticker: string;
  title: string;
  active_events?: number;
  active_markets?: number;
  // Kraken
  active_pairs?: number;
  // Polymarket
  active_conditions?: number;
}

export interface MonitorEvent {
  ticker: string;
  title: string;
  status?: string;
  strike_date?: string | null;
  expected_expiration_time?: string | null;
  market_count?: number;
  exchange?: string;
  // Kraken
  pair_count?: number;
  // Polymarket
  token_count?: number;
  accepting_orders?: boolean;
  end_date?: string | null;
  price_type?: string;
}

export interface MonitorMarket {
  ticker: string;
  title: string;
  status: string;
  close_time: string | null;
  expected_expiration_time?: string | null;
  lifecycle_events?: Array<{
    type: string;
    ts: string;
    metadata: Record<string, unknown>;
  }> | null;
  yes_bid: number | null;
  yes_ask: number | null;
  last: number | null;
  volume: number | null;
  open_interest: number | null;
  snap_at: number | null;
  exchange?: string;
  // Kraken fields
  bid?: number | null;
  ask?: number | null;
  funding_rate?: number | null;
  // Polymarket fields
  best_bid?: number | null;
  best_ask?: number | null;
  spread?: number | null;
  outcome?: string;
  outcome_index?: number;
  price?: string | null;
  price_type?: string;
  mark_price?: string | null;
}

export interface InfoResponse {
  exchange: string;
  environment: string;
  version: string;
}

export interface MeResponse {
  key_prefix: string;
  scopes: string[];
  session_id: number;
  exchange: string;
  environment: string;
  email: string | null;
}

export interface MonitorSearchResult {
  ticker: string;
  title?: string;
  status?: string;
  close_time?: string | null;
  exchange?: string;
  yes_bid?: number | null;
  yes_ask?: number | null;
  last?: number | null;
  volume?: number | null;
  open_interest?: number | null;
}

export interface MonitorSearchResponse {
  results: MonitorSearchResult[];
  count: number;
  query: string;
}

export interface AdminKey {
  prefix: string;
  name: string;
  email?: string;
  scopes: string[];
  rate_limit_tier?: string;
  feeds?: string[];
  expires_at?: string | null;
  last_used_at?: string | null;
}

export interface AdminSession {
  id: number;
  key_prefix: string;
  exchange: string;
  environment: string;
  suspended: boolean;
  created_at?: string;
}

export interface AdminUsersResponse {
  keys: AdminKey[];
  sessions: AdminSession[];
}

/** Harman admin types */
export interface HarmanSession {
  id: number;
  instance: string;
  exchange: string;
  environment: string;
  api_key_prefix: string;
  display_name: string | null;
  max_notional: number;
  open_notional: number;
  open_order_count: number;
  total_fills: number;
  total_settlements: number;
  suspended: boolean;
  created_at: string;
  last_activity: string;
}

export interface ExchangeAuditEntry {
  id: number;
  session_id: number;
  order_id: number | null;
  category: string;
  action: string;
  endpoint: string | null;
  status_code: number | null;
  duration_ms: number | null;
  request: unknown;
  response: unknown;
  outcome: string;
  error_msg: string | null;
  metadata: unknown;
  created_at: string;
}

export interface TimelineEntry {
  ts: string;
  type: string;
  from?: string;
  to?: string;
  actor?: string;
  event?: string;
  details?: string;
  action?: string;
  endpoint?: string;
  status_code?: number;
  duration_ms?: number;
  request?: unknown;
  response?: unknown;
  outcome?: string;
  error_msg?: string;
  metadata?: unknown;
  id?: number;
  price_dollars?: string;
  quantity?: string;
  is_taker?: boolean;
  category?: string;
}

export interface Settlement {
  id: string;
  ticker: string;
  market_result: string;
  revenue_dollars: string;
  created_at: string;
}

export interface OrderTimelineResponse {
  order: Order;
  timeline: TimelineEntry[];
  fills: Fill[];
  settlement: Settlement | null;
}

/** Secmaster types */
export interface SecmasterStats {
  events: { total: number; by_status: Record<string, number>; by_category: Record<string, number> };
  markets: { total: number; by_status: Record<string, number> };
  pairs: { total: number; by_exchange: Record<string, number>; by_market_type: Record<string, number> };
  conditions: { total: number; by_status: Record<string, number>; by_category: Record<string, number> };
}

export interface SecmasterMarket {
  ticker: string;
  eventTicker: string;
  title: string;
  status: string;
  closeTime: string | null;
  volume: number | null;
  volume24h: number | null;
  openInterest: number | null;
}

export interface SecmasterPair {
  pairId: string;
  exchange: string;
  base: string;
  quote: string;
  marketType: string;
  status: string;
  wsName: string;
}

export interface SecmasterCondition {
  conditionId: string;
  question: string;
  status: string;
  category: string | null;
  endDate: string | null;
  tokenCount: number;
}

// Pipeline Engine types
export type PipelineTriggerType = "webhook" | "cron";
export type PipelineRunStatus = "pending" | "running" | "completed" | "failed";
export type PipelineStageType = "sql" | "http" | "gcs_check" | "openrouter" | "email";

export interface Pipeline {
  id: number;
  name: string;
  description: string | null;
  trigger_type: PipelineTriggerType;
  trigger_config: Record<string, unknown>;
  enabled: boolean;
  last_triggered_at: string | null;
  created_at: string;
  updated_at: string;
  last_run_status?: PipelineRunStatus | null;
  last_run_at?: string | null;
  stages?: PipelineStage[];
  webhook_secret?: string;
}

export interface PipelineStage {
  id: number;
  pipeline_id: number;
  position: number;
  name: string;
  stage_type: PipelineStageType;
  config: Record<string, unknown>;
}

export interface PipelineRun {
  id: number;
  pipeline_id: number;
  status: PipelineRunStatus;
  trigger_info: Record<string, unknown> | null;
  started_at: string | null;
  finished_at: string | null;
  created_at: string;
  stage_results?: PipelineStageResult[];
}

export interface PipelineStageResult {
  id: number;
  run_id: number;
  stage_id: number | null;
  status: PipelineRunStatus;
  input: unknown;
  output: unknown;
  error: string | null;
  started_at: string | null;
  finished_at: string | null;
}
