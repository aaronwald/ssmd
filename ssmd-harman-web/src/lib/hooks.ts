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
  getInfo,
  getApiInstance,
} from "./api";

const REFRESH_INTERVAL = 2500;
const METADATA_REFRESH = 60000; // 60s for metadata (categories, series, events)
const LIVE_REFRESH = 2500; // 2.5s for live prices (markets)

/** Prefix SWR key with current instance. Returns null (pauses SWR) when no instance selected. */
function instanceKey(key: string): string | null {
  const inst = getApiInstance();
  return inst ? `${inst}:${key}` : null;
}

export function useHealth() {
  return useSWR(instanceKey("health"), getHealth, {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function useOrders(state?: string) {
  const key = state ? `orders-${state}` : "orders";
  return useSWR(instanceKey(key), () => listOrders(state), {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function useGroups(state?: string) {
  const key = state ? `groups-${state}` : "groups";
  return useSWR(instanceKey(key), () => listGroups(state), {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function useFills() {
  return useSWR(instanceKey("fills"), listFills, {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function useAudit() {
  return useSWR(instanceKey("audit"), listAudit, {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function usePositions() {
  return useSWR(instanceKey("positions"), getPositions, {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function useRisk() {
  return useSWR(instanceKey("risk"), getRisk, {
    refreshInterval: REFRESH_INTERVAL,
  });
}

export function useSnapMap(feed: string = "kalshi") {
  return useSWR(instanceKey(`snap-${feed}`), () => getSnapMap(feed), {
    refreshInterval: REFRESH_INTERVAL,
  });
}

// Monitor hierarchy hooks — tiered refresh rates
export function useCategories() {
  return useSWR(instanceKey("monitor-categories"), getCategories, {
    refreshInterval: METADATA_REFRESH,
  });
}

export function useSeries(category: string | null) {
  return useSWR(
    category ? instanceKey(`monitor-series-${category}`) : null,
    () => getSeries(category!),
    { refreshInterval: METADATA_REFRESH }
  );
}

export function useEvents(series: string | null) {
  return useSWR(
    series ? instanceKey(`monitor-events-${series}`) : null,
    () => getEvents(series!),
    { refreshInterval: METADATA_REFRESH }
  );
}

export function useMarkets(event: string | null) {
  return useSWR(
    event ? instanceKey(`monitor-markets-${event}`) : null,
    () => getMarkets(event!),
    { refreshInterval: LIVE_REFRESH }
  );
}

export function useInfo() {
  return useSWR(instanceKey("info"), getInfo, { revalidateOnFocus: false });
}
