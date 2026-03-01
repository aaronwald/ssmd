"use client";

import { useEffect, type ReactNode } from "react";
import { usePathname } from "next/navigation";
import { useInstance } from "@/lib/instance-context";

/** Redirects to /select when no instance is chosen. Must be inside InstanceProvider. */
export function InstanceGate({ children }: { children: ReactNode }) {
  const { instance, loading } = useInstance();
  const pathname = usePathname();

  useEffect(() => {
    if (!loading && !instance && pathname !== "/select") {
      window.location.href = "/select";
    }
  }, [instance, loading, pathname]);

  if (loading) return null;
  if (!instance && pathname !== "/select") return null;
  return <>{children}</>;
}
