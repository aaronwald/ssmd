"use client";

import { useState } from "react";
import { useGroups } from "@/lib/hooks";
import { cancelGroup } from "@/lib/api";
import { StateBadge } from "@/components/state-badge";
import { CreateBracketForm } from "@/components/create-bracket-form";
import { CreateOcoForm } from "@/components/create-oco-form";
import { useSWRConfig } from "swr";

const stateFilters = [
  { value: "", label: "All" },
  { value: "pending", label: "Pending" },
  { value: "active", label: "Active" },
  { value: "completed", label: "Completed" },
  { value: "cancelled", label: "Cancelled" },
];

export default function GroupsPage() {
  const [filter, setFilter] = useState("");
  const { data: groups, error } = useGroups(filter || undefined);
  const { mutate } = useSWRConfig();
  const [formTab, setFormTab] = useState<"bracket" | "oco">("bracket");

  async function handleCancel(id: number) {
    try {
      await cancelGroup(id);
      mutate((key: string) => typeof key === "string" && key.startsWith("groups"));
    } catch {
      // Error handling is inline
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold">Groups</h1>
        <select
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          className="rounded-md border border-border bg-bg-surface px-3 py-1.5 text-sm text-fg focus:border-accent focus:outline-none"
        >
          {stateFilters.map((f) => (
            <option key={f.value} value={f.value}>{f.label}</option>
          ))}
        </select>
      </div>

      {/* Create form tabs */}
      <div>
        <div className="flex gap-2 mb-3">
          <button
            onClick={() => setFormTab("bracket")}
            className={`rounded-md px-3 py-1 text-sm transition-colors ${formTab === "bracket" ? "bg-accent text-fg" : "text-fg-muted hover:text-fg"}`}
          >
            Bracket
          </button>
          <button
            onClick={() => setFormTab("oco")}
            className={`rounded-md px-3 py-1 text-sm transition-colors ${formTab === "oco" ? "bg-accent text-fg" : "text-fg-muted hover:text-fg"}`}
          >
            OCO
          </button>
        </div>
        {formTab === "bracket" ? <CreateBracketForm /> : <CreateOcoForm />}
      </div>

      {error && <p className="text-sm text-red">Error loading groups: {error.message}</p>}

      <div className="space-y-4">
        {groups && groups.length > 0 ? (
          groups.map((g) => (
            <div key={g.id} className="bg-bg-raised border border-border rounded-lg p-4 space-y-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <span className="font-mono text-fg-muted text-sm">#{g.id}</span>
                  <span className="text-sm uppercase font-medium text-fg">{g.group_type}</span>
                  <StateBadge state={g.state} />
                </div>
                {(g.state === "pending" || g.state === "active") && (
                  <button onClick={() => handleCancel(g.id)} className="text-xs text-red hover:text-red/80">
                    Cancel Group
                  </button>
                )}
              </div>
              {/* Orders within group */}
              <div className="overflow-x-auto">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="text-left text-fg-muted border-b border-border-subtle">
                      <th className="pb-1 pr-3">Role</th>
                      <th className="pb-1 pr-3">Ticker</th>
                      <th className="pb-1 pr-3">Side</th>
                      <th className="pb-1 pr-3">Action</th>
                      <th className="pb-1 pr-3 text-right">Qty</th>
                      <th className="pb-1 pr-3 text-right">Price</th>
                      <th className="pb-1">State</th>
                    </tr>
                  </thead>
                  <tbody>
                    {g.orders.map((o) => (
                      <tr key={o.id} className="border-b border-border-subtle">
                        <td className="py-1 pr-3 text-fg-muted">{o.leg_role || "-"}</td>
                        <td className="py-1 pr-3 font-mono">{o.ticker}</td>
                        <td className="py-1 pr-3 uppercase">{o.side}</td>
                        <td className="py-1 pr-3 uppercase">{o.action}</td>
                        <td className="py-1 pr-3 font-mono text-right">{o.quantity}</td>
                        <td className="py-1 pr-3 font-mono text-right">${o.price_dollars}</td>
                        <td className="py-1"><StateBadge state={o.state} /></td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <div className="text-xs text-fg-subtle">
                Created: {new Date(g.created_at).toLocaleString()}
              </div>
            </div>
          ))
        ) : (
          <div className="bg-bg-raised border border-border rounded-lg p-8 text-center text-fg-subtle text-sm">
            {groups ? "No groups" : "Loading..."}
          </div>
        )}
      </div>
    </div>
  );
}
