import { type NextRequest, NextResponse } from "next/server";

const DATA_TS_URL = process.env.DATA_TS_URL || "";
const DATA_TS_API_KEY = process.env.DATA_TS_API_KEY || "";

export async function GET(
  req: NextRequest,
  { params }: { params: Promise<{ path: string[] }> }
) {
  if (!DATA_TS_URL) {
    return NextResponse.json(
      { error: "DATA_TS_URL not configured" },
      { status: 503 }
    );
  }

  const { path } = await params;
  const targetPath = `/v1/${path.join("/")}`;
  const url = `${DATA_TS_URL}${targetPath}${req.nextUrl.search}`;

  try {
    const res = await fetch(url, {
      headers: {
        Authorization: `Bearer ${DATA_TS_API_KEY}`,
        Accept: "application/json",
      },
      signal: AbortSignal.timeout(30000),
    });

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
