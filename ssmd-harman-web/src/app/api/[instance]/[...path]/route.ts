import { type NextRequest, NextResponse } from "next/server";

function getInstances(): Map<string, string> {
  const raw = process.env.HARMAN_INSTANCES || "";
  const map = new Map<string, string>();
  if (!raw) return map;
  for (const entry of raw.split(",")) {
    const eq = entry.indexOf("=");
    const id = entry.slice(0, eq).trim();
    const host = entry.slice(eq + 1).trim();
    map.set(id, `http://${host}`);
  }
  return map;
}

async function proxy(
  req: NextRequest,
  params: { instance: string; path: string[] }
) {
  const instances = getInstances();
  const backendUrl = instances.get(params.instance);
  if (!backendUrl) {
    return NextResponse.json(
      { error: `unknown instance: ${params.instance}` },
      { status: 404 }
    );
  }

  const targetPath = `/${params.path.join("/")}`;
  const url = new URL(
    `${backendUrl}${targetPath}${req.nextUrl.search}`
  );

  // Forward headers (including CF JWT)
  const headers = new Headers();
  for (const [key, value] of req.headers.entries()) {
    if (
      ["host", "connection", "transfer-encoding", "content-length"].includes(
        key.toLowerCase()
      )
    )
      continue;
    headers.set(key, value);
  }

  const init: RequestInit = {
    method: req.method,
    headers,
    signal: AbortSignal.timeout(30000),
  };

  // Forward body for non-GET/HEAD
  if (req.method !== "GET" && req.method !== "HEAD") {
    init.body = await req.text();
  }

  try {
    const res = await fetch(url.toString(), init);

    const responseHeaders = new Headers();
    for (const [key, value] of res.headers.entries()) {
      if (
        ["transfer-encoding", "content-encoding"].includes(key.toLowerCase())
      )
        continue;
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

export async function GET(
  req: NextRequest,
  { params }: { params: Promise<{ instance: string; path: string[] }> }
) {
  return proxy(req, await params);
}

export async function POST(
  req: NextRequest,
  { params }: { params: Promise<{ instance: string; path: string[] }> }
) {
  return proxy(req, await params);
}

export async function PUT(
  req: NextRequest,
  { params }: { params: Promise<{ instance: string; path: string[] }> }
) {
  return proxy(req, await params);
}

export async function DELETE(
  req: NextRequest,
  { params }: { params: Promise<{ instance: string; path: string[] }> }
) {
  return proxy(req, await params);
}
