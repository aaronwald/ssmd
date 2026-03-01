"use client";

import { useRouter } from "next/navigation";
import { useInstance } from "@/lib/instance-context";
import { setApiInstance } from "@/lib/api";

export default function SelectInstance() {
  const { instances, loading, setInstance } = useInstance();
  const router = useRouter();

  const handleSelect = (id: string) => {
    setApiInstance(id);
    setInstance(id);
    router.push("/");
  };

  if (loading) {
    return (
      <div className="text-center py-20 text-fg-muted">
        Loading instances...
      </div>
    );
  }

  if (instances.length === 0) {
    return (
      <div className="max-w-md mx-auto py-20">
        <h1 className="text-xl font-bold text-fg mb-2">No Instances</h1>
        <p className="text-sm text-fg-muted">
          No harman instances are configured. Set the HARMAN_INSTANCES
          environment variable.
        </p>
      </div>
    );
  }

  return (
    <div className="max-w-md mx-auto py-20">
      <h1 className="text-xl font-bold text-fg mb-2">Select Instance</h1>
      <p className="text-sm text-fg-muted mb-6">
        Choose a harman OMS instance to connect to.
      </p>
      <div className="space-y-3">
        {instances.map((inst) => {
          const envColor =
            inst.environment === "prod"
              ? "border-red-700/50"
              : inst.environment === "demo"
                ? "border-green-700/50"
                : "border-blue-700/50";

          return (
            <button
              key={inst.id}
              onClick={() => handleSelect(inst.id)}
              disabled={!inst.healthy}
              className={`w-full text-left p-4 rounded border transition-colors ${
                inst.healthy
                  ? `${envColor} hover:border-accent bg-bg-raised`
                  : "border-border/50 bg-bg-raised/50 opacity-50 cursor-not-allowed"
              }`}
            >
              <div className="font-mono text-sm font-medium text-fg">
                {inst.id}
              </div>
              <div className="text-xs text-fg-muted mt-1">
                {inst.exchange} &middot; {inst.environment} &middot; v
                {inst.version}
                {!inst.healthy && (
                  <span className="text-red-400 ml-2">offline</span>
                )}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
