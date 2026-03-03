"use client";

interface Node {
  id: string;
  label: string;
  x: number;
  y: number;
  w: number;
  h: number;
  color: string;
  status?: "green" | "yellow" | "red";
}

interface Edge {
  from: string;
  to: string;
  label?: string;
  dashed?: boolean;
  bidir?: boolean;
}

const NODES: Node[] = [
  // Row 1: Data ingestion pipeline
  { id: "kalshi-ws", label: "Kalshi WS", x: 20, y: 20, w: 90, h: 36, color: "#3b82f6" },
  { id: "kraken-ws", label: "Kraken WS", x: 20, y: 64, w: 90, h: 36, color: "#3b82f6" },
  { id: "poly-ws", label: "Poly WS", x: 20, y: 108, w: 90, h: 36, color: "#3b82f6" },
  { id: "connector", label: "Connector", x: 155, y: 56, w: 95, h: 44, color: "#22c55e" },
  { id: "nats", label: "NATS", x: 295, y: 56, w: 80, h: 44, color: "#f97316" },
  { id: "archiver", label: "Archiver", x: 420, y: 20, w: 90, h: 36, color: "#22c55e" },
  { id: "gcs", label: "GCS", x: 555, y: 20, w: 70, h: 36, color: "#fbbf24" },
  { id: "parquet", label: "Parquet-Gen", x: 670, y: 20, w: 105, h: 36, color: "#a78bfa" },

  // Row 2: CDC pipeline
  { id: "cdc", label: "CDC", x: 420, y: 100, w: 70, h: 36, color: "#22c55e" },
  { id: "cache", label: "Cache", x: 535, y: 100, w: 75, h: 36, color: "#22c55e" },
  { id: "redis", label: "Redis", x: 655, y: 100, w: 70, h: 36, color: "#ef4444" },

  // Row 3: Trading + API
  { id: "exchange", label: "Exchange API", x: 20, y: 190, w: 105, h: 40, color: "#3b82f6" },
  { id: "harman", label: "Harman OMS", x: 175, y: 186, w: 120, h: 48, color: "#10b981" },
  { id: "postgres", label: "Postgres", x: 370, y: 190, w: 90, h: 40, color: "#3b82f6" },
  { id: "data-ts", label: "data-ts", x: 530, y: 190, w: 85, h: 40, color: "#a78bfa" },
  { id: "mcp", label: "MCP", x: 675, y: 190, w: 70, h: 40, color: "#fbbf24" },

  // Row 4: Frontend
  { id: "harman-web", label: "Harman Web", x: 175, y: 270, w: 120, h: 40, color: "#60a5fa" },
];

const EDGES: Edge[] = [
  // WS connectors
  { from: "kalshi-ws", to: "connector" },
  { from: "kraken-ws", to: "connector" },
  { from: "poly-ws", to: "connector" },
  // Ingestion
  { from: "connector", to: "nats" },
  { from: "nats", to: "archiver" },
  { from: "archiver", to: "gcs" },
  { from: "gcs", to: "parquet" },
  // CDC
  { from: "nats", to: "cdc" },
  { from: "cdc", to: "cache" },
  { from: "cache", to: "redis" },
  // Trading
  { from: "exchange", to: "harman", bidir: true },
  { from: "harman", to: "postgres" },
  { from: "postgres", to: "data-ts" },
  { from: "data-ts", to: "mcp" },
  { from: "data-ts", to: "redis", dashed: true },
  // Frontend
  { from: "harman-web", to: "harman" },
  { from: "harman-web", to: "data-ts", dashed: true },
];

function getNodeCenter(node: Node): { cx: number; cy: number } {
  return { cx: node.x + node.w / 2, cy: node.y + node.h / 2 };
}

function getEdgePath(from: Node, to: Node): string {
  const a = getNodeCenter(from);
  const b = getNodeCenter(to);
  // Simple straight line — find edge intersection
  const dx = b.cx - a.cx;
  const dy = b.cy - a.cy;
  const len = Math.sqrt(dx * dx + dy * dy);
  if (len === 0) return "";

  const nx = dx / len;
  const ny = dy / len;

  // Start from edge of 'from' box
  const sx = a.cx + nx * (from.w / 2) * (Math.abs(nx) > Math.abs(ny) ? 1 : Math.abs(nx) / Math.abs(ny || 0.01));
  const sy = a.cy + ny * (from.h / 2) * (Math.abs(ny) > Math.abs(nx) ? 1 : Math.abs(ny) / Math.abs(nx || 0.01));

  // End at edge of 'to' box
  const ex = b.cx - nx * (to.w / 2) * (Math.abs(nx) > Math.abs(ny) ? 1 : Math.abs(nx) / Math.abs(ny || 0.01));
  const ey = b.cy - ny * (to.h / 2) * (Math.abs(ny) > Math.abs(nx) ? 1 : Math.abs(ny) / Math.abs(nx || 0.01));

  return `M ${sx} ${sy} L ${ex} ${ey}`;
}

const nodeMap = new Map(NODES.map((n) => [n.id, n]));

export function ArchitectureDiagram() {
  return (
    <div className="bg-bg-raised border border-border rounded-lg p-4 overflow-x-auto">
      <div className="flex items-center justify-between mb-3">
        <h2 className="text-sm font-semibold text-fg">System Architecture</h2>
        <span className="text-xs text-fg-muted">Health indicators are placeholders</span>
      </div>
      <svg
        viewBox="0 0 800 330"
        className="w-full max-w-4xl"
        style={{ minWidth: 600 }}
      >
        <defs>
          <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M 0 0 L 10 5 L 0 10 z" fill="#71717a" />
          </marker>
          <marker id="arrow-rev" viewBox="0 0 10 10" refX="1" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M 10 0 L 0 5 L 10 10 z" fill="#71717a" />
          </marker>
        </defs>

        {/* Edges */}
        {EDGES.map((edge, i) => {
          const from = nodeMap.get(edge.from);
          const to = nodeMap.get(edge.to);
          if (!from || !to) return null;
          const path = getEdgePath(from, to);
          return (
            <path
              key={i}
              d={path}
              stroke="#71717a"
              strokeWidth={1.5}
              fill="none"
              strokeDasharray={edge.dashed ? "4 3" : undefined}
              markerEnd="url(#arrow)"
              markerStart={edge.bidir ? "url(#arrow-rev)" : undefined}
            />
          );
        })}

        {/* Nodes */}
        {NODES.map((node) => (
          <g key={node.id}>
            <rect
              x={node.x}
              y={node.y}
              width={node.w}
              height={node.h}
              rx={6}
              ry={6}
              fill={`${node.color}15`}
              stroke={`${node.color}60`}
              strokeWidth={1.5}
            />
            <text
              x={node.x + node.w / 2}
              y={node.y + node.h / 2 + 1}
              textAnchor="middle"
              dominantBaseline="central"
              fill="#fafafa"
              fontSize={11}
              fontFamily="var(--font-mono), monospace"
              fontWeight={500}
            >
              {node.label}
            </text>
            {/* Health indicator dot */}
            <circle
              cx={node.x + node.w - 6}
              cy={node.y + 6}
              r={3.5}
              fill={node.status === "red" ? "#ef4444" : node.status === "yellow" ? "#fbbf24" : "#22c55e"}
              opacity={0.8}
            />
          </g>
        ))}

        {/* Group labels */}
        <text x={60} y={155} textAnchor="middle" fill="#71717a" fontSize={9} fontFamily="var(--font-sans)">
          Connectors
        </text>
        <text x={335} y={155} textAnchor="middle" fill="#71717a" fontSize={9} fontFamily="var(--font-sans)">
          Streaming
        </text>
        <text x={650} y={155} textAnchor="middle" fill="#71717a" fontSize={9} fontFamily="var(--font-sans)">
          Storage
        </text>
      </svg>
    </div>
  );
}
