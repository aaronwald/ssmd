"use client";

import { Suspense } from "react";
import dynamic from "next/dynamic";

const ActivityContent = dynamic(() => import("@/components/activity-treemap"), {
  ssr: false,
  loading: () => <div className="text-fg-muted">Loading treemap...</div>,
});

export default function ActivityPage() {
  return (
    <Suspense fallback={<div>Loading...</div>}>
      <div className="space-y-4">
        <h1 className="text-xl font-bold">Market Activity</h1>
        <ActivityContent />
      </div>
    </Suspense>
  );
}
