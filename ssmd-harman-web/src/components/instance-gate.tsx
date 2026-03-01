"use client";

import { useEffect, type ReactNode } from "react";
import { usePathname, useRouter } from "next/navigation";
import { useInstance } from "@/lib/instance-context";

/** Pages that don't require an instance selection */
const PUBLIC_PATHS = ["/select", "/settings"];

/** Redirects to /select when no instance is chosen. Must be inside InstanceProvider. */
export function InstanceGate({ children }: { children: ReactNode }) {
  const { instance, loading } = useInstance();
  const pathname = usePathname();
  const router = useRouter();
  const isPublic = PUBLIC_PATHS.includes(pathname);

  useEffect(() => {
    if (!loading && !instance && !isPublic) {
      router.push("/settings");
    }
  }, [instance, loading, pathname, router, isPublic]);

  if (loading) return null;
  if (!instance && !isPublic) return null;
  return <>{children}</>;
}
