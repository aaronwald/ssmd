import { type NextRequest, NextResponse } from "next/server";

const DATA_TS_URL = process.env.DATA_TS_URL || "";
const DATA_TS_API_KEY = process.env.DATA_TS_API_KEY || "";

const ALLOWED_PATH_PREFIXES = [
  "/v1/monitor/",
  "/v1/data/",
  "/v1/secmaster/",
  "/v1/events",
  "/v1/markets",
  "/v1/series",
  "/v1/pairs",
  "/v1/conditions",
  "/v1/fees",
  "/v1/harman/",
  "/v1/health/",
  "/v1/pipelines",
];

function isPathAllowed(path: string): boolean {
  return ALLOWED_PATH_PREFIXES.some((prefix) => path.startsWith(prefix));
}

async function proxy(
  method: string,
  req: NextRequest,
  { params }: { params: Promise<{ path: string[] }> }
) {
  if (!DATA_TS_URL) {
    return NextResponse.json({ error: "DATA_TS_URL not configured" }, { status: 503 });
  }

  const { path } = await params;
  const targetPath = `/v1/${path.join("/")}`;

  if (!isPathAllowed(targetPath)) {
    return NextResponse.json({ error: "Path not allowed" }, { status: 403 });
  }

  const url = `${DATA_TS_URL}${targetPath}${req.nextUrl.search}`;
  const hasBody = method === "POST" || method === "PUT";

  try {
    const res = await fetch(url, {
      method,
      headers: {
        Authorization: `Bearer ${DATA_TS_API_KEY}`,
        ...(hasBody ? { "Content-Type": "application/json" } : {}),
        Accept: "application/json",
      },
      ...(hasBody ? { body: await req.arrayBuffer() } : {}),
      signal: AbortSignal.timeout(30000),
    });

    const responseHeaders = new Headers();
    for (const [key, value] of res.headers.entries()) {
      if (["transfer-encoding", "content-encoding"].includes(key.toLowerCase())) continue;
      responseHeaders.set(key, value);
    }

    const body = await res.arrayBuffer();
    return new NextResponse(body, {
      status: res.status,
      statusText: res.statusText,
      headers: responseHeaders,
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : "proxy error";
    return NextResponse.json({ error: message }, { status: 502 });
  }
}

export const GET = (req: NextRequest, ctx: { params: Promise<{ path: string[] }> }) =>
  proxy("GET", req, ctx);

export const POST = (req: NextRequest, ctx: { params: Promise<{ path: string[] }> }) =>
  proxy("POST", req, ctx);

export const PUT = (req: NextRequest, ctx: { params: Promise<{ path: string[] }> }) =>
  proxy("PUT", req, ctx);

export const DELETE = (req: NextRequest, ctx: { params: Promise<{ path: string[] }> }) =>
  proxy("DELETE", req, ctx);
