export interface SweepResult {
  configId: string;
  params: Record<string, unknown>;
  trades: number;
  wins: number;
  losses: number;
  winRate: number;
  netPnl: number;
  maxDrawdown: number;
  halted: boolean;
  status: "completed" | "failed" | "running" | "pending";
  error?: string;
}

export interface RankOptions {
  sortBy: "pnl" | "winrate" | "drawdown" | "trades";
  minTrades?: number;
  excludeHalted?: boolean;
}

export function rankResults(results: SweepResult[], options: RankOptions): SweepResult[] {
  let filtered = [...results];

  if (options.minTrades !== undefined) {
    filtered = filtered.filter(r => r.trades >= options.minTrades!);
  }

  if (options.excludeHalted) {
    filtered = filtered.filter(r => !r.halted);
  }

  filtered.sort((a, b) => {
    if (a.status === "failed" && b.status !== "failed") return 1;
    if (b.status === "failed" && a.status !== "failed") return -1;

    switch (options.sortBy) {
      case "pnl":
        return b.netPnl - a.netPnl;
      case "winrate":
        return b.winRate - a.winRate;
      case "drawdown":
        return a.maxDrawdown - b.maxDrawdown;
      case "trades":
        return b.trades - a.trades;
      default:
        return b.netPnl - a.netPnl;
    }
  });

  return filtered;
}

export function parseSummaryJson(json: string, configId: string, params: Record<string, unknown>): SweepResult {
  const data = JSON.parse(json);
  const portfolio = data.portfolio ?? {};
  const resultsList = data.results ?? [];

  let totalTrades = 0, totalWins = 0, totalLosses = 0, totalPnl = 0;
  for (const r of resultsList) {
    totalTrades += r.trades ?? 0;
    totalWins += r.wins ?? 0;
    totalLosses += r.losses ?? 0;
    totalPnl += r.netPnl ?? 0;
  }

  return {
    configId,
    params,
    trades: totalTrades,
    wins: totalWins,
    losses: totalLosses,
    winRate: totalTrades > 0 ? totalWins / totalTrades : 0,
    netPnl: totalPnl,
    maxDrawdown: portfolio.drawdownPercent ?? 0,
    halted: portfolio.halted ?? false,
    status: "completed",
  };
}

export function formatResultsTable(results: SweepResult[]): string {
  const lines: string[] = [];
  const header = `${"Rank".padStart(4)} | ${"Config ID".padEnd(28)} | ${"Trades".padStart(6)} | ${"Win%".padStart(5)} | ${"P&L".padStart(8)} | ${"MaxDD".padStart(6)} | ${"Halted".padStart(6)} | Status`;
  const sep = "-".repeat(header.length);

  lines.push(header);
  lines.push(sep);

  results.forEach((r, i) => {
    const rank = String(i + 1).padStart(4);
    const id = r.configId.padEnd(28);
    const trades = String(r.trades).padStart(6);
    const winRate = r.trades > 0 ? `${(r.winRate * 100).toFixed(0)}%`.padStart(5) : "  N/A";
    const pnl = `$${r.netPnl >= 0 ? "+" : ""}${r.netPnl.toFixed(0)}`.padStart(8);
    const dd = `${r.maxDrawdown.toFixed(1)}%`.padStart(6);
    const halted = (r.halted ? "YES" : "no").padStart(6);
    const status = r.status === "failed" ? `FAILED: ${r.error ?? "unknown"}` : r.status;

    lines.push(`${rank} | ${id} | ${trades} | ${winRate} | ${pnl} | ${dd} | ${halted} | ${status}`);
  });

  return lines.join("\n");
}
