import {
  Hero,
  Capabilities,
  Exchanges,
  PipelineEngineering,
  DataQuality,
  Architecture,
  ApiPreview,
  CtaFooter,
} from "./_sections";

// ---------------------------------------------------------------------------
// Landing — in-app rebuild of the old varshtat.com marketing landing page.
//
// Renders inside the standard harman app shell (NavRail + StatsBar +
// InstanceGate from the root layout); no custom layout. Server component —
// the whole page is static, so nothing needs "use client". Section markup +
// content live in ./_sections.tsx.
// ---------------------------------------------------------------------------

// Thin divider between sections, matching the docs page's section dividers.
function Divider() {
  return (
    <div
      className="h-px bg-gradient-to-r from-transparent via-border to-transparent"
      aria-hidden
    />
  );
}

export default function LandingPage() {
  return (
    <div className="max-w-6xl mx-auto px-4 py-10 space-y-20">
      <Hero />
      <Capabilities />
      <Divider />
      <Exchanges />
      <Divider />
      <PipelineEngineering />
      <Divider />
      <DataQuality />
      <Divider />
      <Architecture />
      <Divider />
      <ApiPreview />
      <Divider />
      <CtaFooter />
    </div>
  );
}
