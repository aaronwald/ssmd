"use client";

import type { OrderState, GroupState } from "@/lib/types";

const orderStateColors: Record<OrderState, string> = {
  pending: "bg-yellow/15 text-yellow",
  submitted: "bg-blue-light/15 text-blue-light",
  acknowledged: "bg-green/15 text-green",
  partially_filled: "bg-purple/15 text-purple",
  filled: "bg-emerald/15 text-emerald",
  staged: "bg-slate/15 text-slate",
  pending_cancel: "bg-orange/15 text-orange",
  pending_amend: "bg-orange/15 text-orange",
  pending_decrease: "bg-orange/15 text-orange",
  cancelled: "bg-red/15 text-red",
  rejected: "bg-red/15 text-red",
  expired: "bg-red/15 text-red",
};

const groupStateColors: Record<GroupState, string> = {
  pending: "bg-yellow/15 text-yellow",
  active: "bg-green/15 text-green",
  completed: "bg-emerald/15 text-emerald",
  cancelled: "bg-red/15 text-red",
};

export function StateBadge({ state }: { state: OrderState | GroupState }) {
  const color =
    orderStateColors[state as OrderState] ||
    groupStateColors[state as GroupState] ||
    "bg-fg-subtle/15 text-fg-subtle";

  return (
    <span
      className={`inline-block rounded-md px-2 py-0.5 text-xs font-medium font-mono ${color}`}
    >
      {state}
    </span>
  );
}
