import { NextResponse } from "next/server";

export interface HarmanInstance {
  id: string;
  url: string;
}

/** Parse HARMAN_INSTANCES env var: "kalshi-demo=host:port,kraken-test=host:port" */
function getInstances(): HarmanInstance[] {
  const raw = process.env.HARMAN_INSTANCES || "";
  if (!raw) return [];
  return raw.split(",").map((entry) => {
    const eq = entry.indexOf("=");
    const id = entry.slice(0, eq).trim();
    const host = entry.slice(eq + 1).trim();
    return { id, url: `http://${host}` };
  });
}

export async function GET() {
  const instances = getInstances();

  const results = await Promise.allSettled(
    instances.map(async (inst) => {
      const res = await fetch(`${inst.url}/v1/info`, {
        signal: AbortSignal.timeout(2000),
      });
      if (!res.ok) {
        return {
          id: inst.id,
          exchange: "unknown",
          environment: "unknown",
          version: "unknown",
          healthy: false,
        };
      }
      const info = await res.json();
      return {
        id: inst.id,
        exchange: info.exchange ?? "unknown",
        environment: info.environment ?? "unknown",
        version: info.version ?? "unknown",
        healthy: true,
      };
    })
  );

  const enriched = results.map((r, i) =>
    r.status === "fulfilled"
      ? r.value
      : {
          id: instances[i].id,
          exchange: "unknown",
          environment: "unknown",
          version: "unknown",
          healthy: false,
        }
  );

  return NextResponse.json({ instances: enriched });
}
