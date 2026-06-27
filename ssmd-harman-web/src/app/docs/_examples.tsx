import { Section, CurlBlock } from "./_components";

// ---------------------------------------------------------------------------
// Examples — copy-paste snippets for the Market Data API.
// Rendered after Feeds & Protocols on /docs.
// ---------------------------------------------------------------------------

const PY_SYMBOLS_1M = `import os, requests

API = "https://api.varshtat.com"
H = {"X-API-Key": os.environ["SSMD_API_KEY"]}   # datasets:read key with kraken-spot access

def get(path, **params):
    r = requests.get(f"{API}{path}", params=params, headers=H, timeout=30)
    r.raise_for_status()
    return r.json()

# 1) which kraken-spot symbols currently have live 1m bars
syms = get("/v1/data/ohlcv/1m/symbols", feed="kraken-spot")["symbols"]
print(syms)                       # ['ADA/USDT', 'ATOM/USDT', ..., 'BTC/USDT', ...]

# 2) latest 1m OHLCV bars for one symbol (requests URL-encodes the slash)
bars = get("/v1/data/ohlcv/1m", feed="kraken-spot", sym="BTC/USDT", limit=5)["bars"]
for b in bars:                    # oldest -> newest
    print(b["start_ts_ms"], b["o"], b["h"], b["l"], b["c"], b["v"])`;

export function ExamplesSections() {
  return (
    <Section id="examples" title="Examples">
      <div className="text-sm text-fg-muted space-y-3">
        <p>
          Minimal Python (only <code className="font-mono text-accent">requests</code>): list the{" "}
          <code className="font-mono text-accent">kraken-spot</code> symbols that have live 1-minute
          bars, then read the latest bars for one. Set{" "}
          <code className="font-mono text-accent">SSMD_API_KEY</code> to a key with the{" "}
          <code className="font-mono text-accent">datasets:read</code> scope and{" "}
          <code className="font-mono text-accent">kraken-spot</code> feed access.
        </p>
      </div>
      <div>
        <p className="text-xs text-fg-muted mb-1">
          Python — list symbols + read 1m bars (kraken-spot, BTC/USDT)
        </p>
        <CurlBlock curl={PY_SYMBOLS_1M} />
      </div>
      <p className="text-xs text-fg-subtle">
        Each bar is{" "}
        <code className="font-mono text-accent">{`{ sym, o, h, l, c, v, start_ts_ms, end_ts_ms }`}</code>;{" "}
        <code className="font-mono text-accent">start_ts_ms</code>/
        <code className="font-mono text-accent">end_ts_ms</code> are the minute&apos;s UTC boundaries
        in epoch-ms. kraken-spot symbols are USDT pairs (e.g.{" "}
        <code className="font-mono text-accent">BTC/USDT</code>); if you build the URL by hand,
        URL-encode the slash as <code className="font-mono text-accent">%2F</code>.
      </p>
    </Section>
  );
}
