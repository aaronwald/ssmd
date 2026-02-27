"use client";

import type { RiskResponse } from "@/lib/types";

export function RiskGauge({ risk }: { risk: RiskResponse }) {
  const open = parseFloat(risk.open_notional);
  const max = parseFloat(risk.max_notional);
  const available = parseFloat(risk.available_notional);
  const pct = max > 0 ? (open / max) * 100 : 0;

  let barColor = "bg-green";
  if (pct > 80) barColor = "bg-red";
  else if (pct > 60) barColor = "bg-orange";
  else if (pct > 40) barColor = "bg-yellow";

  return (
    <div className="space-y-2">
      <div className="flex justify-between text-sm">
        <span className="text-fg-muted">Open Notional</span>
        <span className="font-mono text-fg">
          ${open.toFixed(2)} / ${max.toFixed(2)}
        </span>
      </div>
      <div className="h-3 w-full rounded-full bg-bg-surface overflow-hidden">
        <div
          className={`h-full rounded-full transition-all ${barColor}`}
          style={{ width: `${Math.min(pct, 100)}%` }}
        />
      </div>
      <div className="flex justify-between text-xs text-fg-muted">
        <span>{pct.toFixed(1)}% used</span>
        <span>Available: ${available.toFixed(2)}</span>
      </div>
    </div>
  );
}
