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
} from "./api";

const REFRESH_INTERVAL = 5000;

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
