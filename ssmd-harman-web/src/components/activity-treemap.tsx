"use client";

import { useEffect, useRef, useMemo, useState } from "react";
import { useTreemap } from "@/lib/hooks";

// Use inline builds — these bundle WASM as base64 and self-initialize
import perspective from "@finos/perspective/dist/esm/perspective.inline.js";
import "@finos/perspective-viewer/dist/esm/perspective-viewer.inline.js";
import "@finos/perspective-viewer-d3fc";
import type { HTMLPerspectiveViewerElement } from "@finos/perspective-viewer";

// Schema for the Perspective table
const SCHEMA = {
  category: "string",
  series: "string",
  event: "string",
  ticker: "string",
  title: "string",
  volume: "integer",
  open_interest: "integer",
  close_time: "string",
  yes_bid: "float",
  yes_ask: "float",
  last: "float",
};

// Treemap config: group by category→series, size by volume
const VIEWER_CONFIG = {
  plugin: "Treemap",
  group_by: ["category", "series"],
  columns: ["volume"],
  settings: true,
};

// d3fc reads these CSS variables via getComputedStyle() in JS (not CSS fallbacks).
// Without a loaded Perspective theme, d3.color("") returns null and crashes.
// Set the pro-dark theme variables directly on the host element.
const D3FC_THEME_VARS: Record<string, string> = {
  "--d3fc-series": "rgb(71, 120, 194)",
  "--d3fc-series-1": "rgb(71, 120, 194)",
  "--d3fc-series-2": "rgb(204, 120, 48)",
  "--d3fc-series-3": "rgb(158, 84, 192)",
  "--d3fc-series-4": "rgb(51, 150, 153)",
  "--d3fc-series-5": "rgb(102, 114, 143)",
  "--d3fc-series-6": "rgb(211, 103, 189)",
  "--d3fc-series-7": "rgb(109, 124, 77)",
  "--d3fc-series-8": "rgb(221, 99, 103)",
  "--d3fc-series-9": "rgb(120, 104, 206)",
  "--d3fc-series-10": "rgb(23, 166, 123)",
  "--d3fc-full--gradient":
    "linear-gradient(#dd6367 0%, #242526 50%, #3289c8 100%)",
  "--d3fc-positive--gradient":
    "linear-gradient(#242526 0%, #3289c8 100%)",
  "--d3fc-negative--gradient":
    "linear-gradient(#dd6367 0%, #242526 100%)",
  "--d3fc-gridline--color": "#3b3f46",
  "--d3fc-axis-ticks--color": "#c5c9d0",
  "--d3fc-axis--lines": "#61656e",
  "--d3fc-legend--text": "#c5c9d0",
  "--d3fc-treedata--labels": "white",
  "--d3fc-treedata--hover-highlight": "white",
  "--d3fc-tooltip--background": "rgba(42, 44, 47, 1)",
  "--d3fc-tooltip--border-color": "#242526",
  "--d3fc-tooltip--color": "white",
};

function toColumnData(data: any[]) {
  const cols: Record<string, unknown[]> = {
    category: [],
    series: [],
    event: [],
    ticker: [],
    title: [],
    volume: [],
    open_interest: [],
    close_time: [],
    yes_bid: [],
    yes_ask: [],
    last: [],
  };
  for (const m of data) {
    cols.category.push(m.category);
    cols.series.push(m.series);
    cols.event.push(m.event);
    cols.ticker.push(m.ticker);
    cols.title.push(m.title);
    cols.volume.push(m.volume ?? 0);
    cols.open_interest.push(m.open_interest ?? 0);
    cols.close_time.push(m.close_time ?? "");
    cols.yes_bid.push(m.yes_bid ?? null);
    cols.yes_ask.push(m.yes_ask ?? null);
    cols.last.push(m.last ?? null);
  }
  return cols;
}

export default function ActivityTreemap() {
  const { data, error } = useTreemap();
  const containerRef = useRef<HTMLDivElement>(null);
  const viewerRef = useRef<HTMLPerspectiveViewerElement | null>(null);
  const tableRef = useRef<any>(null);
  const clientRef = useRef<any>(null);
  const [tableReady, setTableReady] = useState(false);

  // Initialize Perspective viewer element + client + table on mount
  useEffect(() => {
    const container = containerRef.current;
    if (!container || viewerRef.current) return;

    let cancelled = false;

    // Create the custom element imperatively to avoid JSX type conflicts
    const viewer = document.createElement(
      "perspective-viewer"
    ) as unknown as HTMLPerspectiveViewerElement;
    viewer.style.width = "100%";
    viewer.style.height = "100%";
    // Apply d3fc theme CSS variables on the host element so they cascade
    // into the Shadow DOM where d3fc reads them via getComputedStyle()
    for (const [prop, val] of Object.entries(D3FC_THEME_VARS)) {
      (viewer as unknown as HTMLElement).style.setProperty(prop, val);
    }
    container.appendChild(viewer as unknown as HTMLElement);
    viewerRef.current = viewer;

    async function init() {
      try {
        const client = await perspective.worker();
        if (cancelled) return;
        clientRef.current = client;

        const table = await client.table(SCHEMA as any);
        if (cancelled) return;
        tableRef.current = table;

        await viewer.load(table);
        await viewer.restore(VIEWER_CONFIG);
        if (cancelled) return;

        setTableReady(true);
      } catch (err) {
        console.error("Perspective init failed:", err);
      }
    }

    init();

    return () => {
      cancelled = true;
      if (container.contains(viewer as unknown as HTMLElement)) {
        container.removeChild(viewer as unknown as HTMLElement);
      }
      viewerRef.current = null;
    };
  }, []);

  // Update table data when BOTH table is ready AND data is available.
  // Using tableReady state (not ref) ensures this effect re-runs
  // when the table becomes ready, even if data arrived first.
  useEffect(() => {
    async function update() {
      const table = tableRef.current;
      if (!table || !tableReady || !data || data.length === 0) return;
      try {
        const cols = toColumnData(data);
        await table.replace(cols);
      } catch (err) {
        console.error("Perspective table.replace failed:", err);
      }
    }
    update();
  }, [tableReady, data]);

  const totalVolume = useMemo(() => {
    if (!data) return 0;
    return data.reduce((sum, m) => sum + (m.volume ?? 0), 0);
  }, [data]);

  if (error) {
    return <p className="text-sm text-red">Error loading treemap: {error.message}</p>;
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-4 text-sm text-fg-muted">
        <span>{data ? `${data.length} markets` : "Loading..."}</span>
        {data && <span>Total volume: {totalVolume.toLocaleString()}</span>}
      </div>
      <div
        ref={containerRef}
        className="bg-bg-raised border border-border rounded-lg overflow-hidden"
        style={{ height: "calc(100vh - 180px)" }}
      />
    </div>
  );
}
