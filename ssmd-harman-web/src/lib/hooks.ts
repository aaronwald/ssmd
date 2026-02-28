"use client";

import useSWR from "swr";
import {
  getHealth,
  listOrders,
  listGroups,
  listFills,
  listAudit,
  getPositions,
  getRisk,
  getSnapMap,
  getCategories,
  getSeries,
  getEvents,
  getMarkets,
} from "./api";

const REFRESH_INTERVAL = 5000;
const METADATA_REFRESH = 60000; // 60s for metadata (categories, series, events)
const LIVE_REFRESH = 5000; // 5s for live prices (markets)

export function useHealth() {
  return useSWR("health", getHealth, { refreshInterval: REFRESH_INTERVAL });
}

export function useOrders(state?: string) {
  return useSWR(
    state ? `orders-${state}` : "orders",
    () => listOrders(state),
    { refreshInterval: REFRESH_INTERVAL }
  );
}

export function useGroups(state?: string) {
  return useSWR(
    state ? `groups-${state}` : "groups",
    () => listGroups(state),
    { refreshInterval: REFRESH_INTERVAL }
  );
}

export function useFills() {
  return useSWR("fills", listFills, { refreshInterval: REFRESH_INTERVAL });
}

export function useAudit() {
  return useSWR("audit", listAudit, { refreshInterval: REFRESH_INTERVAL });
}

export function usePositions() {
  return useSWR("positions", getPositions, { refreshInterval: REFRESH_INTERVAL });
}

export function useRisk() {
  return useSWR("risk", getRisk, { refreshInterval: REFRESH_INTERVAL });
}

export function useSnapMap(feed: string = "kalshi") {
  return useSWR(`snap-${feed}`, () => getSnapMap(feed), { refreshInterval: REFRESH_INTERVAL });
}

// Monitor hierarchy hooks — tiered refresh rates
export function useCategories() {
  return useSWR("monitor-categories", getCategories, { refreshInterval: METADATA_REFRESH });
}

export function useSeries(category: string | null) {
  return useSWR(
    category ? `monitor-series-${category}` : null,
    () => getSeries(category!),
    { refreshInterval: METADATA_REFRESH }
  );
}

export function useEvents(series: string | null) {
  return useSWR(
    series ? `monitor-events-${series}` : null,
    () => getEvents(series!),
    { refreshInterval: METADATA_REFRESH }
  );
}

export function useMarkets(event: string | null) {
  return useSWR(
    event ? `monitor-markets-${event}` : null,
    () => getMarkets(event!),
    { refreshInterval: LIVE_REFRESH }
  );
}
