"use client";

const colors: Record<string, string> = {
  green: "bg-green",
  yellow: "bg-yellow",
  red: "bg-red",
};

export function StatusDot({ status }: { status: string }) {
  const color = colors[status] || "bg-fg-subtle";
  return (
    <span className="inline-flex items-center gap-2">
      <span className={`inline-block h-2.5 w-2.5 rounded-full ${color}`} />
      <span className="text-sm text-fg-muted capitalize">{status}</span>
    </span>
  );
}
