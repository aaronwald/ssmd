"use client";

import { useRouter } from "next/navigation";
import { FifteenMinPanel } from "@/components/fifteen-min-panel";

export default function FifteenMinutePage() {
  const router = useRouter();
  return (
    <div className="max-w-2xl space-y-6">
      <h1 className="text-xl font-bold">15-Minute Crypto</h1>
      {/* Clicking a coin opens that market's strike table / order entry on /crypto. */}
      <FifteenMinPanel
        onSelect={(series, event) =>
          router.push(`/crypto?series=${encodeURIComponent(series)}&event=${encodeURIComponent(event)}`)
        }
      />
    </div>
  );
}
