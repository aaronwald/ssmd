export type Side = "yes" | "no";
export type Action = "buy" | "sell";
export type TimeInForce = "gtc" | "ioc";

export type OrderState =
  | "pending"
  | "submitted"
  | "acknowledged"
  | "partially_filled"
  | "filled"
  | "staged"
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
}

/** Monitor hierarchy types */
export interface MonitorCategory {
  name: string;
  event_count: number;
  series_count: number;
}

export interface MonitorSeries {
  ticker: string;
  title: string;
  active_events: number;
  active_markets: number;
}

export interface MonitorEvent {
  ticker: string;
  title: string;
  status: string;
  strike_date: string | null;
  market_count: number;
  exchange?: string;
}

export interface MonitorMarket {
  ticker: string;
  title: string;
  status: string;
  close_time: string | null;
  yes_bid: number | null;
  yes_ask: number | null;
  last: number | null;
  volume: number | null;
  open_interest: number | null;
  exchange?: string;
  // Kraken fields
  bid?: number | null;
  ask?: number | null;
  funding_rate?: number | null;
  // Polymarket fields
  best_bid?: number | null;
  best_ask?: number | null;
  spread?: number | null;
}

export interface InfoResponse {
  exchange: string;
  environment: string;
  version: string;
}
