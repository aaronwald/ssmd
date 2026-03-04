"use client";

import Link from "next/link";
import { useHealth, useRisk, useOrders, useFills } from "@/lib/hooks";
import { useInstance } from "@/lib/instance-context";

export function StatsBar() {
  const { instance } = useInstance();
  const { data: health } = useHealth();
  const { data: risk } = useRisk();
  const { data: openOrders } = useOrders("open");
  const { data: fills } = useFills();

  if (!instance) {
    return (
      <div className="flex items-center h-10 px-4 border-b border-border bg-bg-raised text-xs text-fg-subtle">
        No instance selected
      </div>
    );
  }

  const isHealthy = health ? (health.status === "ok" || health.status === "healthy") : false;
  const healthStatus = health ? (isHealthy ? "green" : "red") : "yellow";
  const healthColors: Record<string, string> = { green: "bg-green", red: "bg-red", yellow: "bg-yellow" };

  const openN = risk ? parseFloat(risk.open_notional) : 0;
  const maxN = risk ? parseFloat(risk.max_notional) : 0;
  const pct = maxN > 0 ? (openN / maxN) * 100 : 0;
  const riskColor = pct > 80 ? "text-red" : pct > 50 ? "text-orange" : "text-fg";

  const orderCount = openOrders?.length ?? 0;
  const fillCount = fills?.length ?? 0;

  return (
    <div className="flex items-center h-10 px-4 gap-6 border-b border-border bg-bg-raised text-xs shrink-0">
      {/* Health */}
      <Link href="/" className="flex items-center gap-2 hover:text-fg text-fg-muted">
        <span
          className={`inline-block h-2 w-2 rounded-full ${healthColors[healthStatus]}`}
          style={healthStatus === "green" ? { animation: "pulse-dot 2s ease-in-out infinite" } : undefined}
        />
        <span>{health ? (isHealthy ? "Healthy" : health.status) : "connecting"}</span>
      </Link>

      <span className="w-px h-4 bg-border" />

      {/* Risk */}
      <Link href="/" className="flex items-center gap-2 hover:text-fg text-fg-muted">
        <span>Risk:</span>
        <span className={`font-mono ${riskColor}`}>
          ${openN.toFixed(0)}/${maxN.toFixed(0)}
        </span>
        <span className="text-fg-subtle">({pct.toFixed(0)}%)</span>
      </Link>

      <span className="w-px h-4 bg-border" />

      {/* Open Orders */}
      <Link href="/orders" className="flex items-center gap-2 hover:text-fg text-fg-muted">
        <span className="font-mono">{orderCount}</span>
        <span>Open</span>
      </Link>

      <span className="w-px h-4 bg-border" />

      {/* Fills */}
      <Link href="/fills" className="flex items-center gap-2 hover:text-fg text-fg-muted">
        <span className="font-mono">{fillCount}</span>
        <span>Fills</span>
      </Link>
    </div>
  );
}
