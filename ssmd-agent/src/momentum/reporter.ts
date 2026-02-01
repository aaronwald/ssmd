import type { Position, ClosedPosition, PositionManager } from "./position-manager.ts";
import type { SignalResult } from "./signals/types.ts";

export class Reporter {
  private summaryIntervalSec: number;
  private lastSummaryTs = 0;
  quiet = false;

  constructor(summaryIntervalMinutes: number) {
    this.summaryIntervalSec = summaryIntervalMinutes * 60;
  }

  logEntry(signals: SignalResult[], pos: Position): void {
    const time = new Date(pos.entryTime * 1000).toISOString();
    const signalNames = signals.map(s => `${s.name}(${s.score.toFixed(2)})`).join("+");
    const reasons = signals.map(s => s.reason).join("; ");
    console.log(
      `[ENTRY] ${time} signals=${signalNames} ticker=${pos.ticker} side=${pos.side} price=${pos.entryPrice} contracts=${pos.contracts} cost=$${pos.entryCost.toFixed(2)} | ${reasons}`
    );
  }

  logExit(closed: ClosedPosition): void {
    const time = new Date(closed.exitTime * 1000).toISOString();
    const pnlStr = closed.pnl >= 0 ? `+$${closed.pnl.toFixed(2)}` : `-$${Math.abs(closed.pnl).toFixed(2)}`;
    console.log(
      `[EXIT]  ${time} model=${closed.position.model} ticker=${closed.position.ticker} side=${closed.position.side} entry=${closed.position.entryPrice} exit=${closed.exitPrice} reason=${closed.reason} pnl=${pnlStr} fees=$${closed.fees.toFixed(2)}`
    );
  }

  logActivation(ticker: string, ts: number): void {
    if (this.quiet) return;
    const time = new Date(ts * 1000).toISOString();
    console.log(`[ACTIVATED] ${time} ticker=${ticker}`);
  }

  logHalt(pm: PositionManager): void {
    const summary = pm.getSummary();
    console.log(`\n*** TRADING HALTED - Drawdown limit reached ***`);
    console.log(`Balance: $${summary.balance.toFixed(2)} (${summary.drawdownPercent.toFixed(1)}% drawdown)`);
    console.log(`Total trades: ${summary.totalTrades} | Wins: ${summary.wins} | Losses: ${summary.losses}\n`);
  }

  maybePrintSummary(pm: PositionManager, currentTs: number): void {
    if (this.lastSummaryTs === 0) {
      this.lastSummaryTs = currentTs;
      return;
    }

    if (this.quiet) return;
    if (currentTs - this.lastSummaryTs < this.summaryIntervalSec) return;

    this.lastSummaryTs = currentTs;
    this.printSummary(pm);
  }

  printSummary(pm: PositionManager): void {
    const summary = pm.getSummary();

    const byModel = new Map<string, { trades: number; wins: number; losses: number; pnl: number }>();
    for (const c of pm.closedPositions) {
      const m = c.position.model;
      const stats = byModel.get(m) ?? { trades: 0, wins: 0, losses: 0, pnl: 0 };
      stats.trades++;
      if (c.pnl > 0) stats.wins++;
      else stats.losses++;
      stats.pnl += c.pnl;
      byModel.set(m, stats);
    }

    console.log(`\n--- Summary ---`);
    console.log(`${"Model".padEnd(22)} ${"Trades".padStart(6)} ${"Wins".padStart(6)} ${"Losses".padStart(6)} ${"Win%".padStart(6)} ${"Net P&L".padStart(10)}`);

    for (const [model, stats] of byModel) {
      const winPct = stats.trades > 0 ? ((stats.wins / stats.trades) * 100).toFixed(0) : "0";
      const pnlStr = stats.pnl >= 0 ? `+$${stats.pnl.toFixed(2)}` : `-$${Math.abs(stats.pnl).toFixed(2)}`;
      console.log(`${model.padEnd(22)} ${String(stats.trades).padStart(6)} ${String(stats.wins).padStart(6)} ${String(stats.losses).padStart(6)} ${(winPct + "%").padStart(6)} ${pnlStr.padStart(10)}`);
    }

    const pnlStr = summary.totalPnl >= 0 ? `+$${summary.totalPnl.toFixed(2)}` : `-$${Math.abs(summary.totalPnl).toFixed(2)}`;
    console.log(`Balance: $${summary.balance.toFixed(2)} | Open: ${pm.openPositions.length} | Drawdown: ${summary.drawdownPercent.toFixed(1)}% | P&L: ${pnlStr}`);
    if (pm.isHalted) console.log(`*** HALTED ***`);
    console.log(``);
  }
}
