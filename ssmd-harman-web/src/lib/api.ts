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
} from "./types";

const BASE_URL =
  process.env.NEXT_PUBLIC_HARMAN_URL || "http://localhost:8080";
const TOKEN = process.env.NEXT_PUBLIC_HARMAN_TOKEN || "";

async function request<T>(
  path: string,
  options: RequestInit = {}
): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(TOKEN ? { Authorization: `Bearer ${TOKEN}` } : {}),
    ...(options.headers as Record<string, string>),
  };

  const res = await fetch(`${BASE_URL}${path}`, {
    ...options,
    headers,
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
export const createOrder = (order: CreateOrderRequest) =>
  request<Order>("/v1/orders", { method: "POST", body: JSON.stringify(order) });

export const cancelOrder = (id: number) =>
  request<void>(`/v1/orders/${id}`, { method: "DELETE" });

export const amendOrder = (id: number, body: { new_price_dollars?: string; new_quantity?: string }) =>
  request<Order>(`/v1/orders/${id}/amend`, { method: "POST", body: JSON.stringify(body) });

export const decreaseOrder = (id: number, body: { reduce_by: string }) =>
  request<Order>(`/v1/orders/${id}/decrease`, { method: "POST", body: JSON.stringify(body) });

export const createBracket = (req: CreateBracketRequest) =>
  request<OrderGroup>("/v1/groups/bracket", { method: "POST", body: JSON.stringify(req) });

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
  request<void>("/v1/orders/mass-cancel", { method: "POST" });

// Ticker search (secmaster)
export const searchTickers = (q: string) =>
  request<{ tickers: string[] }>(`/v1/tickers?q=${encodeURIComponent(q)}`);
