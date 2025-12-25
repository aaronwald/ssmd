// ssmd-agent - Health check stub
// Future: LangGraph signal runtime, NATS subscription

const PORT = parseInt(Deno.env.get("PORT") ?? "8080");

function handler(req: Request): Response {
  const url = new URL(req.url);

  if (url.pathname === "/health" || url.pathname === "/healthz") {
    return new Response(JSON.stringify({ status: "ok" }), {
      headers: { "content-type": "application/json" },
    });
  }

  if (url.pathname === "/") {
    return new Response(JSON.stringify({
      service: "ssmd-agent",
      version: "0.1.0",
    }), {
      headers: { "content-type": "application/json" },
    });
  }

  return new Response("Not Found", { status: 404 });
}

console.log(`ssmd-agent listening on :${PORT}`);
Deno.serve({ port: PORT }, handler);
