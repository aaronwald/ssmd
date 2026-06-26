import Link from "next/link";

// ---------------------------------------------------------------------------
// Landing page sections.
//
// Lightweight in-app rebuild of the old varshtat.com marketing landing,
// restyled with harman-web's design tokens. No framer-motion, no
// react-router-dom, no mdx CodeBlock — static markup only (plus Tailwind
// hover transitions). The `_` filename prefix keeps Next from treating this
// as a route, mirroring src/app/docs/_components.tsx.
// ---------------------------------------------------------------------------

// Shared heading treatment: subtle vertical gradient for scale-contrast
// hierarchy, built from harman foreground tokens.
const headingGradient =
  "bg-gradient-to-b from-fg to-fg-subtle bg-clip-text text-transparent";

function SectionHeading({
  title,
  subtitle,
}: {
  title: string;
  subtitle?: string;
}) {
  return (
    <div className="mb-10 text-center">
      <h2
        className={`text-3xl sm:text-4xl font-bold tracking-tight ${headingGradient}`}
      >
        {title}
      </h2>
      {subtitle && (
        <p className="mt-3 text-base sm:text-lg text-fg-muted max-w-2xl mx-auto">
          {subtitle}
        </p>
      )}
    </div>
  );
}

function CheckIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="currentColor" className="h-4 w-4">
      <path
        fillRule="evenodd"
        d="M16.704 4.153a.75.75 0 01.143 1.052l-8 10.5a.75.75 0 01-1.127.075l-4.5-4.5a.75.75 0 011.06-1.06l3.894 3.893 7.48-9.817a.75.75 0 011.05-.143z"
        clipRule="evenodd"
      />
    </svg>
  );
}

const primaryButton =
  "inline-flex items-center justify-center rounded-lg bg-accent px-6 py-3 text-sm font-semibold text-white transition-all hover:bg-accent-hover hover:-translate-y-0.5 active:translate-y-0";

const secondaryButton =
  "inline-flex items-center justify-center rounded-lg border border-border px-6 py-3 text-sm font-semibold text-fg-muted transition-all hover:text-fg hover:border-fg-subtle hover:bg-bg-surface hover:-translate-y-0.5 active:translate-y-0";

// ---------------------------------------------------------------------------
// 1. Hero
// ---------------------------------------------------------------------------

export function Hero() {
  return (
    <section className="relative overflow-hidden rounded-lg border border-border-subtle bg-bg-raised px-4 py-20 sm:py-28">
      {/* Static dot-grid texture, faded toward the edges. */}
      <div
        className="pointer-events-none absolute inset-0 opacity-[0.35]"
        aria-hidden
      >
        <div
          className="absolute inset-0"
          style={{
            backgroundImage:
              "radial-gradient(circle, var(--color-fg-subtle) 0.5px, transparent 0.5px)",
            backgroundSize: "32px 32px",
          }}
        />
        <div
          className="absolute inset-0"
          style={{
            background:
              "radial-gradient(ellipse 60% 50% at 50% 40%, transparent 0%, var(--color-bg-raised) 100%)",
          }}
        />
      </div>

      <div className="relative z-10 flex flex-col items-center text-center">
        <h1
          className={`text-4xl sm:text-5xl md:text-6xl font-bold tracking-tight leading-[1.1] ${headingGradient}`}
        >
          Market data workshop
        </h1>
        <p className="mt-5 max-w-2xl text-base sm:text-lg text-fg-muted leading-relaxed">
          Streaming trades, quotes, and lifecycle events from Kalshi and
          Kraken&nbsp;&mdash; archived to Parquet, queryable via API and MCP.
        </p>
        <div className="mt-8 flex flex-col sm:flex-row gap-4">
          <a href="mailto:api@varshtat.com" className={primaryButton}>
            Get API Access
          </a>
          <a href="mailto:pipelines@varshtat.com" className={secondaryButton}>
            Build a Custom Pipeline
          </a>
        </div>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// 2. Capabilities — "What We Build"
// ---------------------------------------------------------------------------

interface Capability {
  title: string;
  description: string;
  icon: React.ReactNode;
}

const capabilities: Capability[] = [
  {
    title: "Multi-Exchange Ingestion",
    description:
      "Real-time WebSocket streams from Kalshi and Kraken via NATS JetStream.",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.5} className="h-6 w-6">
        <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 13.5l10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75z" />
      </svg>
    ),
  },
  {
    title: "Query-Ready Archives",
    description:
      "Hourly Parquet files on GCS. Query with DuckDB or download via signed URLs.",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.5} className="h-6 w-6">
        <path strokeLinecap="round" strokeLinejoin="round" d="M20.25 6.375c0 2.278-3.694 4.125-8.25 4.125S3.75 8.653 3.75 6.375m16.5 0c0-2.278-3.694-4.125-8.25-4.125S3.75 4.097 3.75 6.375m16.5 0v11.25c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125V6.375m16.5 0v3.75c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125v-3.75" />
      </svg>
    ),
  },
  {
    title: "REST API",
    description:
      "Market metadata, prices, trades, and aggregation. Scoped API keys.",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.5} className="h-6 w-6">
        <path strokeLinecap="round" strokeLinejoin="round" d="M6.75 7.5l3 2.25-3 2.25m4.5 0h3m-9 8.25h13.5A2.25 2.25 0 0021 18V6a2.25 2.25 0 00-2.25-2.25H5.25A2.25 2.25 0 003 6v12a2.25 2.25 0 002.25 2.25z" />
      </svg>
    ),
  },
  {
    title: "MCP Integration",
    description:
      "30+ tools for querying market data from any MCP-compatible AI client.",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.5} className="h-6 w-6">
        <path strokeLinecap="round" strokeLinejoin="round" d="M13.19 8.688a4.5 4.5 0 011.242 7.244l-4.5 4.5a4.5 4.5 0 01-6.364-6.364l1.757-1.757m13.35-.622l1.757-1.757a4.5 4.5 0 00-6.364-6.364l-4.5 4.5a4.5 4.5 0 001.242 7.244" />
      </svg>
    ),
  },
  {
    title: "Custom Pipeline Engineering",
    description:
      "Bespoke data pipelines — custom schemas, aggregation windows, automated delivery.",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.5} className="h-6 w-6">
        <path strokeLinecap="round" strokeLinejoin="round" d="M9.75 3.104v5.714a2.25 2.25 0 01-.659 1.591L5 14.5M9.75 3.104c-.251.023-.498.05-.75.082m.75-.082a24.301 24.301 0 014.5 0m0 0v5.714c0 .597.237 1.17.659 1.591L19.8 15.3M14.25 3.104c.251.023.498.05.75.082M19.8 15.3l-1.57.393A9.065 9.065 0 0112 15a9.065 9.065 0 00-6.23.693L5 14.5m14.8.8l1.402 1.402c1.232 1.232.65 3.318-1.067 3.611A48.309 48.309 0 0112 21c-2.773 0-5.491-.235-8.135-.687-1.718-.293-2.3-2.379-1.067-3.61L5 14.5" />
      </svg>
    ),
  },
  {
    title: "Data Quality & Monitoring",
    description:
      "Automated DQ scoring, freshness alerts, and pipeline health dashboards.",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.5} className="h-6 w-6">
        <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
      </svg>
    ),
  },
];

export function Capabilities() {
  return (
    <section className="px-4">
      <SectionHeading
        title="What We Build"
        subtitle="End-to-end infrastructure for capturing, storing, and querying exchange data at scale."
      />
      <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
        {capabilities.map((card) => (
          <div
            key={card.title}
            className="group rounded-lg border border-border-subtle bg-bg-raised p-6 transition-colors hover:border-border hover:bg-bg-surface"
          >
            <div className="mb-4 inline-flex items-center justify-center rounded-lg bg-accent/10 p-3 text-accent">
              {card.icon}
            </div>
            <h3 className="text-lg font-semibold text-fg mb-1.5">
              {card.title}
            </h3>
            <p className="text-sm leading-relaxed text-fg-muted">
              {card.description}
            </p>
          </div>
        ))}
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// 3. Exchanges — "Supported Exchanges"
// ---------------------------------------------------------------------------

interface ExchangeCard {
  name: string;
  badge: string;
  channels: string[];
  stat: string;
}

const exchanges: ExchangeCard[] = [
  {
    name: "Kalshi",
    badge: "Prediction Markets",
    channels: ["ticker", "trade", "orderbook_delta", "lifecycle"],
    stat: "28M+ rows archived",
  },
  {
    name: "Kraken Futures",
    badge: "Crypto Perpetuals",
    channels: ["ticker", "trade"],
    stat: "7.8M+ rows archived",
  },
  {
    name: "Kraken Spot",
    badge: "Crypto Spot",
    channels: ["ticker", "trade"],
    stat: "35 USDT pairs",
  },
];

export function Exchanges() {
  return (
    <section className="px-4">
      <SectionHeading title="Supported Exchanges" />
      <div className="grid grid-cols-1 md:grid-cols-3 gap-5">
        {exchanges.map((ex) => (
          <div
            key={ex.name}
            className="group rounded-lg border border-border-subtle bg-bg-raised p-6 transition-colors hover:border-accent/40"
          >
            <div className="mb-4">
              <h3 className="text-xl font-semibold text-fg">{ex.name}</h3>
              <span className="mt-1.5 inline-block rounded-full bg-accent/10 px-3 py-0.5 text-xs font-medium text-accent">
                {ex.badge}
              </span>
            </div>
            <div className="mb-5 flex flex-wrap gap-2">
              {ex.channels.map((ch) => (
                <span
                  key={ch}
                  className="rounded-md bg-bg-surface px-2.5 py-1 font-mono text-xs text-fg-subtle"
                >
                  {ch}
                </span>
              ))}
            </div>
            <div className="border-t border-border-subtle pt-4">
              <span className="text-sm font-medium text-green">{ex.stat}</span>
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// 4. Pipeline Engineering — "Custom Data Pipelines"
// ---------------------------------------------------------------------------

interface Feature {
  title: string;
  description: string;
}

const pipelineFeatures: Feature[] = [
  {
    title: "Rapid Deployment",
    description: "From concept to production in days, not months.",
  },
  {
    title: "Custom Aggregation",
    description:
      "OHLCV bars, volume profiles, and trade summaries at any granularity.",
  },
  {
    title: "Tailored Schemas",
    description: "Flat Parquet, enriched columns, your naming conventions.",
  },
  {
    title: "Automated Delivery",
    description:
      "Daily runs with notifications. Data lands in GCS, queryable instantly.",
  },
  {
    title: "Source Flexibility",
    description:
      "WebSocket streams, REST APIs, or both. Multiple independent datasets.",
  },
];

function FeatureRow({ feature }: { feature: Feature }) {
  return (
    <div className="flex items-start gap-4">
      <div className="mt-0.5 flex-shrink-0 rounded-full bg-green/10 p-1.5 text-green">
        <CheckIcon />
      </div>
      <div>
        <h3 className="text-base font-semibold text-fg">{feature.title}</h3>
        <p className="mt-0.5 text-sm leading-relaxed text-fg-muted">
          {feature.description}
        </p>
      </div>
    </div>
  );
}

export function PipelineEngineering() {
  return (
    <section className="px-4">
      <div className="mx-auto max-w-2xl">
        <SectionHeading
          title="Custom Data Pipelines"
          subtitle="From raw exchange feeds to delivery-ready datasets."
        />
        <div className="space-y-6">
          {pipelineFeatures.map((f) => (
            <FeatureRow key={f.title} feature={f} />
          ))}
        </div>
        <div className="mt-10 text-center">
          <a href="mailto:pipelines@varshtat.com" className={primaryButton}>
            Talk to Us
          </a>
        </div>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// 5. Data Quality — "Data Quality You Can Verify"
// ---------------------------------------------------------------------------

const dqFeatures: Feature[] = [
  {
    title: "Automated DQ Scoring",
    description: "Every pipeline scored daily on completeness and freshness.",
  },
  {
    title: "Freshness Monitoring",
    description: "Stale pipelines flagged within hours with automated alerts.",
  },
  {
    title: "Gap Detection",
    description:
      "Source-to-output row count validation and timeline completeness.",
  },
  {
    title: "Health Dashboards",
    description: "Prometheus metrics and daily email reports.",
  },
];

const dqMetrics: { value: string; label: string }[] = [
  { value: "28M+", label: "trades archived" },
  { value: "3", label: "exchanges monitored" },
  { value: "24/7", label: "uptime target" },
  { value: "< 7h", label: "freshness SLA" },
];

export function DataQuality() {
  return (
    <section className="px-4">
      <SectionHeading title="Data Quality You Can Verify" />
      <div className="mx-auto max-w-2xl space-y-6 mb-12">
        {dqFeatures.map((f) => (
          <FeatureRow key={f.title} feature={f} />
        ))}
      </div>
      <div className="grid grid-cols-2 md:grid-cols-4 gap-6 rounded-lg border border-border-subtle bg-bg-raised p-8">
        {dqMetrics.map((m) => (
          <div key={m.label} className="text-center">
            <div className="text-4xl sm:text-5xl font-bold text-fg font-mono">
              {m.value}
            </div>
            <div className="mt-2 text-sm text-fg-muted">{m.label}</div>
          </div>
        ))}
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// 6. Architecture — "How It Works"
// ---------------------------------------------------------------------------

interface FlowNode {
  label: string;
  icon: React.ReactNode;
}

const flowNodes: FlowNode[] = [
  {
    label: "Exchange",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.5} className="h-6 w-6">
        <path strokeLinecap="round" strokeLinejoin="round" d="M12 21a9.004 9.004 0 008.716-6.747M12 21a9.004 9.004 0 01-8.716-6.747M12 21c2.485 0 4.5-4.03 4.5-9S14.485 3 12 3m0 18c-2.485 0-4.5-4.03-4.5-9S9.515 3 12 3m0 0a8.997 8.997 0 017.843 4.582M12 3a8.997 8.997 0 00-7.843 4.582m15.686 0A11.953 11.953 0 0112 10.5c-2.998 0-5.74-1.1-7.843-2.918m15.686 0A8.959 8.959 0 0121 12c0 .778-.099 1.533-.284 2.253m0 0A17.919 17.919 0 0112 16.5c-3.162 0-6.133-.815-8.716-2.247m0 0A9.015 9.015 0 013 12c0-1.605.42-3.113 1.157-4.418" />
      </svg>
    ),
  },
  {
    label: "WebSocket",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.5} className="h-6 w-6">
        <path strokeLinecap="round" strokeLinejoin="round" d="M7.5 21L3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5" />
      </svg>
    ),
  },
  {
    label: "NATS JetStream",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.5} className="h-6 w-6">
        <path strokeLinecap="round" strokeLinejoin="round" d="M6.429 9.75L2.25 12l4.179 2.25m0-4.5l5.571 3 5.571-3m-11.142 0L2.25 7.5 12 2.25l9.75 5.25-4.179 2.25m0 0L21.75 12l-4.179 2.25m0 0L21.75 16.5 12 21.75 2.25 16.5l4.179-2.25m0 0l5.571 3 5.571-3" />
      </svg>
    ),
  },
  {
    label: "Archiver",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.5} className="h-6 w-6">
        <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
      </svg>
    ),
  },
  {
    label: "Parquet / GCS",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.5} className="h-6 w-6">
        <path strokeLinecap="round" strokeLinejoin="round" d="M20.25 6.375c0 2.278-3.694 4.125-8.25 4.125S3.75 8.653 3.75 6.375m16.5 0c0-2.278-3.694-4.125-8.25-4.125S3.75 4.097 3.75 6.375m16.5 0v11.25c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125V6.375m16.5 0v3.75c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125v-3.75" />
      </svg>
    ),
  },
  {
    label: "API / MCP",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.5} className="h-6 w-6">
        <path strokeLinecap="round" strokeLinejoin="round" d="M6.75 7.5l3 2.25-3 2.25m4.5 0h3m-9 8.25h13.5A2.25 2.25 0 0021 18V6a2.25 2.25 0 00-2.25-2.25H5.25A2.25 2.25 0 003 6v12a2.25 2.25 0 002.25 2.25z" />
      </svg>
    ),
  },
];

function FlowArrow() {
  return (
    <>
      {/* Horizontal arrow on md+ */}
      <div className="hidden md:flex items-center justify-center w-8 shrink-0">
        <svg viewBox="0 0 40 12" className="w-8 h-4 text-fg-subtle">
          <line x1="0" y1="6" x2="32" y2="6" stroke="currentColor" strokeWidth="2" strokeDasharray="4 3" />
          <path d="M30 2 L38 6 L30 10" fill="none" stroke="currentColor" strokeWidth="2" />
        </svg>
      </div>
      {/* Vertical arrow on mobile */}
      <div className="flex md:hidden items-center justify-center h-6 shrink-0">
        <svg viewBox="0 0 12 28" className="w-3 h-7 text-fg-subtle">
          <line x1="6" y1="0" x2="6" y2="22" stroke="currentColor" strokeWidth="2" strokeDasharray="4 3" />
          <path d="M2 20 L6 26 L10 20" fill="none" stroke="currentColor" strokeWidth="2" />
        </svg>
      </div>
    </>
  );
}

export function Architecture() {
  return (
    <section className="px-4">
      <SectionHeading
        title="How It Works"
        subtitle="Exchange data flows through a durable, real-time pipeline into query-ready storage."
      />
      <div className="flex flex-col md:flex-row items-center justify-center">
        {flowNodes.map((node, i) => (
          <div key={node.label} className="contents">
            <div className="flex flex-col items-center gap-3 rounded-lg border border-border-subtle bg-bg-raised px-6 py-5 min-w-[8.5rem] transition-colors hover:border-border hover:bg-bg-surface">
              <div className="inline-flex items-center justify-center rounded-lg bg-accent/10 p-3 text-accent">
                {node.icon}
              </div>
              <span className="text-sm font-medium text-fg-muted whitespace-nowrap">
                {node.label}
              </span>
            </div>
            {i < flowNodes.length - 1 && <FlowArrow />}
          </div>
        ))}
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// 7. API Preview — "Try the API"
// ---------------------------------------------------------------------------

const curlCommand = `curl -H "X-API-Key: $API_KEY" \\
  "https://api.varshtat.com/v1/data/trades?feed=kalshi&date=2026-03-09&limit=3"`;

const jsonResponse = `{
  "trades": [
    {
      "ticker": "KXBTCD-26MAR0916-T89999.99",
      "count": 847,
      "volume": 12450,
      "avg_price": 0.52
    }
  ],
  "feed": "kalshi",
  "date": "2026-03-09"
}`;

function CodeSample({ label, code }: { label: string; code: string }) {
  return (
    <div className="mt-3">
      <p className="text-xs text-fg-muted mb-1">{label}</p>
      <pre className="bg-bg font-mono text-xs text-fg-muted p-3 rounded border border-border overflow-x-auto whitespace-pre-wrap">
        {code}
      </pre>
    </div>
  );
}

export function ApiPreview() {
  return (
    <section className="px-4">
      <div className="mx-auto max-w-3xl">
        <SectionHeading title="Try the API" />
        <CodeSample label="Request" code={curlCommand} />
        <CodeSample label="Response" code={jsonResponse} />
        <div className="mt-6 text-center">
          <Link
            href="/docs"
            className="inline-flex items-center gap-1.5 text-sm font-medium text-accent hover:text-accent-hover transition-colors"
          >
            See full API reference
            <svg viewBox="0 0 20 20" fill="currentColor" className="h-4 w-4">
              <path fillRule="evenodd" d="M3 10a.75.75 0 01.75-.75h10.638L10.23 5.29a.75.75 0 111.04-1.08l5.5 5.25a.75.75 0 010 1.08l-5.5 5.25a.75.75 0 11-1.04-1.08l4.158-3.96H3.75A.75.75 0 013 10z" clipRule="evenodd" />
            </svg>
          </Link>
        </div>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// 8. CTA Footer
// ---------------------------------------------------------------------------

interface CtaCard {
  title: string;
  description: string;
  cta: string;
  href: string;
  primary: boolean;
}

const ctaCards: CtaCard[] = [
  {
    title: "Get API Access",
    description:
      "Streaming market data, Parquet downloads, and trade aggregation via REST.",
    cta: "Request API Key",
    href: "mailto:api@varshtat.com",
    primary: true,
  },
  {
    title: "Build a Custom Pipeline",
    description:
      "Custom market data pipelines — from raw exchange feeds to delivery-ready datasets.",
    cta: "Get in Touch",
    href: "mailto:pipelines@varshtat.com",
    primary: false,
  },
];

export function CtaFooter() {
  return (
    <section className="px-4">
      <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
        {ctaCards.map((card) => (
          <div
            key={card.title}
            className="rounded-lg border border-border-subtle bg-bg-raised p-8 flex flex-col"
          >
            <h3 className="text-xl sm:text-2xl font-bold text-fg mb-2">
              {card.title}
            </h3>
            <p className="text-sm sm:text-base leading-relaxed text-fg-muted mb-6 flex-1">
              {card.description}
            </p>
            <div>
              <a
                href={card.href}
                className={card.primary ? primaryButton : secondaryButton}
              >
                {card.cta}
              </a>
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}
