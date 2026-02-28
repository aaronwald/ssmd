"use client";

import { useEffect, useRef, useMemo } from "react";
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

export default function ActivityTreemap() {
  const { data, error } = useTreemap();
  const containerRef = useRef<HTMLDivElement>(null);
  const viewerRef = useRef<HTMLPerspectiveViewerElement | null>(null);
  const tableRef = useRef<any>(null);
  const clientRef = useRef<any>(null);

  // Convert SWR data to column-oriented format for Perspective
  const columnData = useMemo(() => {
    if (!data || data.length === 0) return null;
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
  }, [data]);

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
    viewer.className = "perspective-viewer-material-dark";
    container.appendChild(viewer as unknown as HTMLElement);
    viewerRef.current = viewer;

    async function init() {
      const client = await perspective.worker();
      if (cancelled) return;
      clientRef.current = client;

      const table = await client.table(SCHEMA as any);
      if (cancelled) return;
      tableRef.current = table;

      await viewer.load(table);
      await viewer.restore(VIEWER_CONFIG);
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

  // Update table data when SWR data changes
  useEffect(() => {
    async function update() {
      const table = tableRef.current;
      if (!table || !columnData) return;
      await table.replace(columnData);
    }
    update();
  }, [columnData]);

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
