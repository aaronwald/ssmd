"use client";

import { type ReactNode } from "react";
import { useInstance } from "@/lib/instance-context";

/** Blocks rendering until an instance is selected. Instance switching is handled by the nav badge. */
export function InstanceGate({ children }: { children: ReactNode }) {
  const { instance, loading } = useInstance();

  if (loading) return null;
  if (!instance) {
    return (
      <div className="py-20 text-center text-fg-muted text-sm">
        No healthy harman instance available. Check HARMAN_INSTANCES configuration.
      </div>
    );
  }
  return <>{children}</>;
}
