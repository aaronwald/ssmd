"use client";

/** Inline dot showing snap data staleness. Yellow >30s, red >120s. */
export function SnapAgeDot({ snapAt }: { snapAt: number | null }) {
  if (snapAt == null) {
    return <span className="inline-block w-2 h-2 rounded-full bg-fg-subtle ml-1" title="No snap timestamp" />;
  }

  const ageMs = Date.now() - snapAt;
  const ageSec = Math.round(ageMs / 1000);

  let color = "bg-green";
  let label = `${ageSec}s ago`;
  if (ageSec > 120) {
    color = "bg-red";
    label = `Stale: ${ageSec}s ago`;
  } else if (ageSec > 30) {
    color = "bg-yellow";
    label = `${ageSec}s ago`;
  }

  return <span className={`inline-block w-2 h-2 rounded-full ${color} ml-1`} title={label} />;
}
