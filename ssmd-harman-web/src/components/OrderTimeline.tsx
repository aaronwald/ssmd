"use client";

import { useState } from "react";
import { useOrderTimeline } from "@/lib/hooks";
import { StateBadge } from "@/components/state-badge";
import type { TimelineEntry, OrderState } from "@/lib/types";

const typeColors: Record<string, { bg: string; text: string; label: string }> = {
  state_change: { bg: "bg-accent/15", text: "text-accent", label: "State" },
  rest_call: { bg: "bg-green/15", text: "text-green", label: "REST" },
  exchange_call: { bg: "bg-green/15", text: "text-green", label: "REST" },
  ws_event: { bg: "bg-blue-light/15", text: "text-blue-light", label: "WS" },
  fill: { bg: "bg-purple/15", text: "text-purple", label: "Fill" },
  fallback: { bg: "bg-orange/15", text: "text-orange", label: "Fallback" },
  reconciliation: { bg: "bg-yellow/15", text: "text-yellow", label: "Recon" },
  recovery: { bg: "bg-yellow/15", text: "text-yellow", label: "Recovery" },
  risk: { bg: "bg-red/15", text: "text-red", label: "Risk" },
};

function TypeBadge({ type, outcome }: { type: string; outcome?: string }) {
  const style = typeColors[type] || { bg: "bg-fg-subtle/15", text: "text-fg-subtle", label: type };
  const isError = outcome === "error" || outcome === "not_found" || outcome === "timeout" || outcome === "rate_limited";
  const bg = isError ? "bg-red/15" : style.bg;
  const text = isError ? "text-red" : style.text;
  return (
    <span className={`inline-block rounded-md px-2 py-0.5 text-xs font-medium font-mono ${bg} ${text}`}>
      {style.label}
    </span>
  );
}

function EntryDetail({ entry }: { entry: TimelineEntry }) {
  const [expanded, setExpanded] = useState(false);

  if (entry.type === "state_change") {
    return (
      <div className="flex items-center gap-1.5">
        {entry.from && <StateBadge state={entry.from as OrderState} />}
        <span className="text-fg-subtle">&rarr;</span>
        {entry.to && <StateBadge state={entry.to as OrderState} />}
        {entry.actor && <span className="text-xs text-fg-muted ml-2">by {entry.actor}</span>}
      </div>
    );
  }

  if (entry.type === "fill") {
    return (
      <div className="flex items-center gap-3 text-xs">
        <span className="font-mono text-purple">
          ${entry.price_dollars} x {entry.quantity}
        </span>
        {entry.is_taker !== undefined && (
          <span className={entry.is_taker ? "text-yellow" : "text-fg-muted"}>
            {entry.is_taker ? "taker" : "maker"}
          </span>
        )}
      </div>
    );
  }

  if (entry.type === "fallback") {
    return (
      <div className="text-xs">
        <span className="text-orange font-mono">{entry.action}</span>
        {entry.outcome && (
          <span className={`ml-2 ${entry.outcome === "success" ? "text-green" : "text-red"}`}>
            {entry.outcome}
          </span>
        )}
        {entry.metadata != null && (
          <span className="text-fg-muted ml-2">{JSON.stringify(entry.metadata)}</span>
        )}
      </div>
    );
  }

  // rest_call / exchange_call / ws_event
  const hasPayload = entry.request != null || entry.response != null;
  return (
    <div className="text-xs space-y-1">
      <div className="flex items-center gap-2 flex-wrap">
        <span className="font-mono text-fg">{entry.action}</span>
        {entry.endpoint && <span className="text-fg-muted">{entry.endpoint}</span>}
        {entry.status_code && (
          <span className={`font-mono ${entry.status_code >= 400 ? "text-red" : "text-green"}`}>
            {entry.status_code}
          </span>
        )}
        {entry.duration_ms !== undefined && entry.duration_ms !== null && (
          <span className="text-fg-muted">{entry.duration_ms}ms</span>
        )}
        {entry.outcome && entry.outcome !== "success" && (
          <span className="text-red">{entry.outcome}</span>
        )}
        {entry.error_msg && (
          <span className="text-red truncate max-w-xs" title={entry.error_msg}>{entry.error_msg}</span>
        )}
      </div>
      {hasPayload && (
        <button
          onClick={() => setExpanded(!expanded)}
          className="text-fg-muted hover:text-fg transition-colors"
        >
          {expanded ? "Hide" : "Show"} payload {expanded ? "▲" : "▼"}
        </button>
      )}
      {expanded && (
        <div className="bg-bg rounded border border-border-subtle p-2 mt-1 space-y-2 overflow-x-auto max-h-48 overflow-y-auto">
          {entry.request != null && (
            <div>
              <span className="text-fg-muted text-[10px] uppercase">Request</span>
              <pre className="text-[11px] text-fg font-mono whitespace-pre-wrap break-all">
                {typeof entry.request === "string" ? entry.request : JSON.stringify(entry.request, null, 2)}
              </pre>
            </div>
          )}
          {entry.response != null && (
            <div>
              <span className="text-fg-muted text-[10px] uppercase">Response</span>
              <pre className="text-[11px] text-fg font-mono whitespace-pre-wrap break-all">
                {typeof entry.response === "string" ? entry.response : JSON.stringify(entry.response, null, 2)}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function OrderTimeline({ orderId, instance }: { orderId: number; instance?: string }) {
  const { data, error } = useOrderTimeline(orderId, instance);

  if (error) {
    return <p className="text-xs text-red py-2">Error loading timeline: {error.message}</p>;
  }

  if (!data) {
    return <p className="text-xs text-fg-muted py-2">Loading timeline...</p>;
  }

  const { order, timeline, settlement } = data;

  return (
    <div className="space-y-3">
      {/* Order summary */}
      <div className="flex items-center gap-3 text-xs flex-wrap">
        <span className="font-mono text-fg">{order.ticker}</span>
        <span className="uppercase">{order.side} {order.action}</span>
        <span className="font-mono">{order.quantity} @ ${order.price_dollars}</span>
        <StateBadge state={order.state} />
        {order.cancel_reason && <span className="text-fg-muted">({order.cancel_reason})</span>}
      </div>

      {/* Timeline */}
      <div className="relative pl-6 border-l border-border-subtle space-y-0">
        {timeline.map((entry, i) => (
          <div key={i} className="relative pb-3 last:pb-0">
            {/* Dot on the timeline line */}
            <div
              className={`absolute -left-[25px] top-1 h-2.5 w-2.5 rounded-full border-2 border-bg-raised ${
                entry.outcome === "error" || entry.outcome === "not_found"
                  ? "bg-red"
                  : entry.type === "fill"
                  ? "bg-purple"
                  : entry.type === "state_change"
                  ? "bg-accent"
                  : entry.type === "fallback"
                  ? "bg-orange"
                  : "bg-green"
              }`}
            />
            <div className="flex items-start gap-2">
              <span className="text-[10px] text-fg-muted font-mono shrink-0 w-20 pt-0.5">
                {new Date(entry.ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}
              </span>
              <TypeBadge type={entry.type} outcome={entry.outcome} />
              <div className="flex-1 min-w-0">
                <EntryDetail entry={entry} />
              </div>
            </div>
          </div>
        ))}
      </div>

      {/* Settlement info */}
      {settlement != null && (
        <div className="text-xs bg-emerald/10 text-emerald rounded px-3 py-2">
          Settlement: {JSON.stringify(settlement)}
        </div>
      )}

      {timeline.length === 0 && (
        <p className="text-xs text-fg-muted">No timeline entries yet.</p>
      )}
    </div>
  );
}
