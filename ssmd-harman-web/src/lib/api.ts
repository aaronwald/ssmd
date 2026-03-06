import type {
  Order,
  OrderGroup,
  Fill,
  AuditEntry,
  PositionsView,
  RiskResponse,
  CreateOrderRequest,
  CreateBracketRequest,
  CreateOcoRequest,
  HealthResponse,
  SnapResponse,
  NormalizedSnapshot,
  MonitorCategory,
  MonitorSeries,
  MonitorEvent,
  MonitorMarket,
  MonitorSearchResponse,
  InfoResponse,
  MeResponse,
  AdminUsersResponse,
  HarmanSession,
  ExchangeAuditEntry,
  OrderTimelineResponse,
  SecmasterStats,
  SecmasterMarket,
  SecmasterPair,
  SecmasterCondition,
} from "./types";

// Dynamic instance routing — set via InstanceProvider
let _currentInstance: string | null = null;

export function setApiInstance(instance: string) {
  _currentInstance = instance;
}

export function getApiInstance(): string | null {
  return _currentInstance;
}

async function request<T>(
  path: string,
  options: RequestInit = {},
  instanceOverride?: string
): Promise<T> {
  const inst = instanceOverride || _currentInstance;
  if (!inst) throw new Error("No harman instance selected");
  const baseUrl = `/api/${inst}`;

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(options.headers as Record<string, string>),
  };

  const res = await fetch(`${baseUrl}${path}`, {
    ...options,
    headers,
    credentials: "include",
  });

  if (!res.ok) {
    const body = await res.text();
    throw new Error(`${res.status} ${res.statusText}: ${body}`);
  }

  if (res.status === 204) return undefined as T;
  return res.json();
}

/** Fetch from data-ts (global market data — not instance-scoped). */
async function dataRequest<T>(
  path: string,
  options: RequestInit = {}
): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(options.headers as Record<string, string>),
  };

  const res = await fetch(`/api/data${path}`, {
    ...options,
    headers,
    credentials: "include",
  });

  if (!res.ok) {
    const body = await res.text();
    throw new Error(`${res.status} ${res.statusText}: ${body}`);
  }

  if (res.status === 204) return undefined as T;
  return res.json();
}

// Read endpoints — API wraps lists in envelope keys
export const listOrders = async (state?: string): Promise<Order[]> => {
  const res = await request<{ orders: Order[] }>(`/v1/orders${state ? `?state=${state}` : ""}`);
  return res.orders;
};

export const getOrder = (id: number) =>
  request<Order>(`/v1/orders/${id}`);

export const listGroups = async (state?: string): Promise<OrderGroup[]> => {
  const res = await request<{ groups: OrderGroup[] }>(`/v1/groups${state ? `?state=${state}` : ""}`);
  return res.groups;
};

export const getGroup = (id: number) =>
  request<OrderGroup>(`/v1/groups/${id}`);

export const listFills = async (): Promise<Fill[]> => {
  const res = await request<{ fills: Fill[] }>("/v1/fills");
  return res.fills;
};

export const listAudit = async (): Promise<AuditEntry[]> => {
  const res = await request<{ audit: AuditEntry[] }>("/v1/audit");
  return res.audit;
};

// Write endpoints
export const createOrder = (order: CreateOrderRequest, instanceId?: string) =>
  request<Order>("/v1/orders", { method: "POST", body: JSON.stringify(order) }, instanceId);

export const cancelOrder = (id: number) =>
  request<void>(`/v1/orders/${id}`, { method: "DELETE" });

export const amendOrder = (id: number, body: { new_price_dollars?: string; new_quantity?: string }) =>
  request<Order>(`/v1/orders/${id}/amend`, { method: "POST", body: JSON.stringify(body) });

export const decreaseOrder = (id: number, body: { reduce_by: string }) =>
  request<Order>(`/v1/orders/${id}/decrease`, { method: "POST", body: JSON.stringify(body) });

export const createBracket = (req: CreateBracketRequest, instanceId?: string) =>
  request<OrderGroup>("/v1/groups/bracket", { method: "POST", body: JSON.stringify(req) }, instanceId);

export const createOco = (req: CreateOcoRequest) =>
  request<OrderGroup>("/v1/groups/oco", { method: "POST", body: JSON.stringify(req) });

export const cancelGroup = (id: number) =>
  request<void>(`/v1/groups/${id}`, { method: "DELETE" });

// Admin endpoints
export const getHealth = () =>
  request<HealthResponse>("/health");

export const getPositions = () =>
  request<PositionsView>("/v1/admin/positions");

export const getRisk = () =>
  request<RiskResponse>("/v1/admin/risk");

export const pump = () =>
  request<void>("/v1/admin/pump", { method: "POST" });

export const reconcile = () =>
  request<void>("/v1/admin/reconcile", { method: "POST" });

export const resume = () =>
  request<void>("/v1/admin/resume", { method: "POST" });

export const massCancel = () =>
  request<void>("/v1/admin/mass-cancel", { method: "POST", body: JSON.stringify({ confirm: true }) });

// Ticker search (secmaster)
export const searchTickers = (q: string) =>
  request<{ tickers: string[]; degraded?: boolean }>(`/v1/tickers?q=${encodeURIComponent(q)}`);

// Market data (snap) — returns normalized snapshots keyed by ticker
export const getSnapMap = async (feed: string = "kalshi"): Promise<Map<string, NormalizedSnapshot>> => {
  const raw = await request<SnapResponse>(`/v1/snap?feed=${encodeURIComponent(feed)}`);
  const map = new Map<string, NormalizedSnapshot>();
  for (const s of raw.snapshots) {
    const ticker = s._ticker;
    const msg = s.msg;
    if (!ticker || !msg) continue;
    const yesBid = msg.yes_bid_dollars != null ? parseFloat(msg.yes_bid_dollars) : (msg.yes_bid != null ? msg.yes_bid / 100 : null);
    const yesAsk = msg.yes_ask_dollars != null ? parseFloat(msg.yes_ask_dollars) : (msg.yes_ask != null ? msg.yes_ask / 100 : null);
    const last = msg.price_dollars != null ? parseFloat(msg.price_dollars) : (msg.price != null ? msg.price / 100 : null);
    // Extract _snap_at timestamp injected by snap service
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const snapAt = (s as any)._snap_at ?? (msg as any)._snap_at ?? null;
    map.set(ticker, { ticker, yesBid, yesAsk, last, snapAt: typeof snapAt === "number" ? snapAt : null });
  }
  return map;
};

// Monitor hierarchy endpoints — global market data via data-ts
export const getCategories = async (): Promise<MonitorCategory[]> => {
  const res = await dataRequest<{ categories: MonitorCategory[] }>("/monitor/categories");
  return res.categories;
};

export const getSeries = async (category: string): Promise<MonitorSeries[]> => {
  const res = await dataRequest<{ series: MonitorSeries[] }>(`/monitor/series?category=${encodeURIComponent(category)}`);
  return res.series;
};

export const getEvents = async (series: string): Promise<MonitorEvent[]> => {
  const res = await dataRequest<{ events: MonitorEvent[] }>(`/monitor/events?series=${encodeURIComponent(series)}`);
  return res.events;
};

export const getMarkets = async (event: string): Promise<MonitorMarket[]> => {
  const res = await dataRequest<{ markets: MonitorMarket[] }>(`/monitor/markets?event=${encodeURIComponent(event)}`);
  return res.markets;
};

// Info endpoint (public, no auth)
export const getInfo = () =>
  request<InfoResponse>("/v1/info");

// Me endpoint — returns auth context for current user
export const getMe = () =>
  request<MeResponse>("/v1/me");

// Monitor search — search by series or outcomes
export const searchMonitorMarkets = async (q: string, type?: string, exchange?: string, limit?: number): Promise<MonitorSearchResponse> => {
  const params = new URLSearchParams({ q });
  if (type) params.set("type", type);
  if (exchange) params.set("exchange", exchange);
  if (limit) params.set("limit", String(limit));
  return dataRequest<MonitorSearchResponse>(`/monitor/search?${params.toString()}`);
};

// Admin users endpoint
export const getAdminUsers = () =>
  request<AdminUsersResponse>("/v1/admin/users");

// Harman admin endpoints (via data-ts)
export const getHarmanSessions = async (): Promise<HarmanSession[]> => {
  const res = await dataRequest<{ sessions: HarmanSession[] }>("/harman/sessions");
  return res.sessions;
};

export const getSessionOrders = async (sessionId: number, instance?: string): Promise<Order[]> => {
  const qs = instance ? `?limit=100&instance=${encodeURIComponent(instance)}` : "?limit=100";
  const res = await dataRequest<{ orders: Order[] }>(`/harman/sessions/${sessionId}/orders${qs}`);
  return res.orders;
};

export const getOrderTimeline = async (orderId: number, instance?: string): Promise<OrderTimelineResponse> => {
  const qs = instance ? `?instance=${encodeURIComponent(instance)}` : "";
  return dataRequest<OrderTimelineResponse>(`/harman/orders/${orderId}/timeline${qs}`);
};

export const getExchangeAudit = async (sessionId: number, instance?: string): Promise<ExchangeAuditEntry[]> => {
  const qs = instance ? `?limit=200&instance=${encodeURIComponent(instance)}` : "?limit=200";
  const res = await dataRequest<{ entries: ExchangeAuditEntry[] }>(`/harman/sessions/${sessionId}/exchange-audit${qs}`);
  return res.entries;
};

// Secmaster endpoints (via data-ts)
export const getSecmasterStats = () =>
  dataRequest<SecmasterStats>("/secmaster/stats");

export const getSecmasterMarkets = async (params: {
  status?: string;
  series?: string;
  category?: string;
  close_within_hours?: number;
  limit?: number;
}): Promise<SecmasterMarket[]> => {
  const qs = new URLSearchParams();
  if (params.status) qs.set("status", params.status);
  if (params.series) qs.set("series", params.series);
  if (params.category) qs.set("category", params.category);
  if (params.close_within_hours) qs.set("close_within_hours", String(params.close_within_hours));
  qs.set("limit", String(params.limit ?? 100));
  const res = await dataRequest<{ markets: SecmasterMarket[] }>(`/markets?${qs.toString()}`);
  return res.markets;
};

export const getSecmasterPairs = async (params: {
  status?: string;
  base?: string;
  market_type?: string;
  limit?: number;
}): Promise<SecmasterPair[]> => {
  const qs = new URLSearchParams();
  if (params.status) qs.set("status", params.status);
  if (params.base) qs.set("base", params.base);
  if (params.market_type) qs.set("market_type", params.market_type);
  qs.set("limit", String(params.limit ?? 100));
  const res = await dataRequest<{ pairs: SecmasterPair[] }>(`/pairs?${qs.toString()}`);
  return res.pairs;
};

export const getSecmasterConditions = async (params: {
  status?: string;
  category?: string;
  limit?: number;
}): Promise<SecmasterCondition[]> => {
  const qs = new URLSearchParams();
  if (params.status) qs.set("status", params.status);
  if (params.category) qs.set("category", params.category);
  qs.set("limit", String(params.limit ?? 100));
  const res = await dataRequest<{ conditions: SecmasterCondition[] }>(`/conditions?${qs.toString()}`);
  return res.conditions;
};

