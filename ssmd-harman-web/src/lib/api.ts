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
  WhoamiResponse,
  AdminUsersResponse,
  HarmanSession,
  ExchangeAuditEntry,
  OrderTimelineResponse,
  SecmasterStats,
  SecmasterMarket,
  SecmasterPair,
  SecmasterCondition,
  Pipeline,
  PipelineRun,
  DayFilesResponse,
  DataDownloadResponse,
  DataCatalogResponse,
  CreateKeyRequest,
  CreateKeyResponse,
  RotateWelcomeResponse,
  KeyUsage,
  KeyUsageResponse,
  KeyRequestCounts,
  KeyRequestsResponse,
} from "./types";

// Dynamic instance routing — set via InstanceProvider
let _currentInstance: string | null = null;

export function setApiInstance(instance: string) {
  _currentInstance = instance;
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

  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 10000);

  let res: Response;
  try {
    res = await fetch(`${baseUrl}${path}`, {
      ...options,
      headers,
      credentials: "include",
      signal: controller.signal,
    });
  } catch (err) {
    clearTimeout(timeout);
    if (err instanceof DOMException && err.name === "AbortError") {
      throw new Error("Request timed out — server may be unavailable");
    }
    throw new Error("Network error — server may be unavailable");
  }
  clearTimeout(timeout);

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
export const getSnapMap = async (feed: string, tickers: string[]): Promise<Map<string, NormalizedSnapshot>> => {
  if (tickers.length === 0) return new Map();
  const url = `/v1/snap?feed=${encodeURIComponent(feed)}&tickers=${encodeURIComponent(tickers.join(","))}`;
  const raw = await request<SnapResponse>(url);
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

// Me endpoint — resolves the caller's identity via the global data-ts whoami endpoint.
// Uses dataRequest (no OMS instance required) so it works for researchers without a
// harman session. Maps WhoamiResponse onto MeResponse: fields used by the app are
// scopes and email; OMS-only fields (key_prefix, session_id, exchange, environment)
// are provided as safe defaults since no OMS page displays them from this hook.
export const getMe = async (): Promise<MeResponse> => {
  // dataRequest propagates HTTP errors as thrown exceptions — not caught here so
  // SWR receives the error and can surface it to consumers.
  const whoami = await dataRequest<WhoamiResponse>("/data/whoami");
  if (!whoami || typeof whoami !== "object") {
    throw new Error("whoami: unexpected empty response from /v1/data/whoami");
  }
  if (!Array.isArray(whoami.scopes)) {
    throw new Error("whoami: missing or invalid scopes field");
  }
  if (typeof whoami.email !== "string" || whoami.email.length === 0) {
    throw new Error("whoami: missing or invalid email field");
  }
  return {
    email: whoami.email,
    scopes: whoami.scopes,
    // OMS-only fields — not accessed by any component; safe defaults provided.
    key_prefix: "",
    session_id: 0,
    exchange: "",
    environment: "",
  };
};

// Monitor search — search by series or outcomes
export const searchMonitorMarkets = async (q: string, type?: string, exchange?: string, limit?: number): Promise<MonitorSearchResponse> => {
  const params = new URLSearchParams({ q });
  if (type) params.set("type", type);
  if (exchange) params.set("exchange", exchange);
  if (limit) params.set("limit", String(limit));
  return dataRequest<MonitorSearchResponse>(`/monitor/search?${params.toString()}`);
};

// Daily files — global data-ts endpoints (not instance-scoped).
// NOTE: dataRequest prepends "/api/data" and the proxy prepends "/v1", so the
// path here omits the "/v1" segment (e.g. "/data/day" -> "/v1/data/day").
export const getDataCatalog = () =>
  dataRequest<DataCatalogResponse>("/data/catalog");

export const getDayFiles = (date: string) =>
  dataRequest<DayFilesResponse>(`/data/day?date=${encodeURIComponent(date)}`);

export const getDataDownload = (feed: string, date: string) =>
  dataRequest<DataDownloadResponse>(
    `/data/download?feed=${encodeURIComponent(feed)}&from=${encodeURIComponent(date)}&to=${encodeURIComponent(date)}`,
  );

// Admin users endpoint
export const getAdminUsers = () =>
  request<AdminUsersResponse>("/v1/admin/users");

// API-key usage stats (via data-ts, read-only, admin scope).
// The data proxy strips the "/v1" segment, so paths omit it here.
export const getKeyUsage = async (): Promise<KeyUsage[]> => {
  const res = await dataRequest<KeyUsageResponse>("/keys/usage");
  if (!res || !Array.isArray(res.usage)) {
    throw new Error("keys/usage: missing or invalid usage array");
  }
  return res.usage;
};

export const getKeyRequests = async (): Promise<KeyRequestCounts[]> => {
  const res = await dataRequest<KeyRequestsResponse>("/keys/requests");
  if (!res || !Array.isArray(res.keys)) {
    throw new Error("keys/requests: missing or invalid keys array");
  }
  return res.keys;
};

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

// Pipeline endpoints (via data-ts)
export const listPipelines = () =>
  dataRequest<Pipeline[]>("/pipelines");

export const getPipeline = (id: number) =>
  dataRequest<Pipeline>(`/pipelines/${id}`);

export const createPipeline = (body: {
  name: string;
  description?: string;
  trigger_type: string;
  trigger_config?: Record<string, unknown>;
  stages?: Array<{ name: string; stage_type: string; config: Record<string, unknown> }>;
}) =>
  dataRequest<Pipeline>("/pipelines", { method: "POST", body: JSON.stringify(body) });

export const updatePipeline = (id: number, body: Record<string, unknown>) =>
  dataRequest<Pipeline>(`/pipelines/${id}`, { method: "PUT", body: JSON.stringify(body) });

export const deletePipeline = (id: number) =>
  dataRequest<{ deleted: boolean }>(`/pipelines/${id}`, { method: "DELETE" });

export const triggerPipeline = (id: number, payload?: Record<string, unknown>) =>
  dataRequest<{ run_id: number; status: string }>(`/pipelines/${id}/run`, {
    method: "POST",
    ...(payload && Object.keys(payload).length > 0 ? { body: JSON.stringify(payload) } : {}),
  });

export const getPipelineRuns = (id: number) =>
  dataRequest<PipelineRun[]>(`/pipelines/${id}/runs`);

export const getPipelineRunDetail = (runId: number) =>
  dataRequest<PipelineRun>(`/pipelines/runs/${runId}`);

// Key management endpoints
export const createKeyWelcome = (payload: CreateKeyRequest): Promise<CreateKeyResponse> => {
  if (!payload.name || !payload.userEmail) {
    return Promise.reject(new Error("name and userEmail are required"));
  }
  return request<CreateKeyResponse>("/v1/keys", { method: "POST", body: JSON.stringify(payload) });
};

export const rotateWelcome = (prefix: string, recipient?: string): Promise<RotateWelcomeResponse> => {
  if (!prefix) {
    return Promise.reject(new Error("prefix is required"));
  }
  const body: { recipient?: string } = {};
  if (recipient && recipient.trim()) {
    body.recipient = recipient.trim();
  }
  return request<RotateWelcomeResponse>(`/v1/keys/${encodeURIComponent(prefix)}/rotate-welcome`, {
    method: "POST",
    body: JSON.stringify(body),
  });
};
