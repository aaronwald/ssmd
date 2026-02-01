import type { MarketState } from "../market-state.ts";
import type { Signal, SignalResult } from "./types.ts";

export interface TradeClusteringConfig {
  windowSec: number;
  quietThresholdSec: number;
  burstGapSec: number;
  minBurstTrades: number;
  weight: number;
}

const ZERO: SignalResult = { name: "trade-clustering", score: 0, confidence: 0, reason: "" };

interface Burst {
  trades: { side: string; count: number; price: number; ts: number }[];
  startTs: number;
  endTs: number;
}

export class TradeClustering implements Signal {
  readonly name = "trade-clustering";
  private readonly config: TradeClusteringConfig;

  constructor(config: TradeClusteringConfig) {
    this.config = config;
  }

  evaluate(state: MarketState): SignalResult {
    const trades = state.getRecentTrades(this.config.windowSec);
    if (trades.length < this.config.minBurstTrades) return ZERO;

    // Find bursts: sequences where gap < burstGapSec, preceded by quiet >= quietThresholdSec
    const bursts: Burst[] = [];
    let currentBurst: Burst | null = null;

    for (let i = 0; i < trades.length; i++) {
      if (i === 0) {
        currentBurst = { trades: [trades[i]], startTs: trades[i].ts, endTs: trades[i].ts };
        continue;
      }

      const gap = trades[i].ts - trades[i - 1].ts;

      if (gap <= this.config.burstGapSec) {
        // Continue current burst
        currentBurst!.trades.push(trades[i]);
        currentBurst!.endTs = trades[i].ts;
      } else {
        // End current burst, check if it qualifies
        if (currentBurst && currentBurst.trades.length >= this.config.minBurstTrades) {
          // Check for quiet period before this burst
          const burstStart = currentBurst.startTs;
          const prevTradeTs = this.findPrevTradeTs(trades, currentBurst.trades[0], burstStart);
          const quietPeriod = burstStart - prevTradeTs;
          if (quietPeriod >= this.config.quietThresholdSec) {
            bursts.push(currentBurst);
          }
        }
        // Start new burst
        currentBurst = { trades: [trades[i]], startTs: trades[i].ts, endTs: trades[i].ts };
      }
    }

    // Check final burst
    if (currentBurst && currentBurst.trades.length >= this.config.minBurstTrades) {
      const burstStart = currentBurst.startTs;
      const prevTradeTs = this.findPrevTradeTs(trades, currentBurst.trades[0], burstStart);
      const quietPeriod = burstStart - prevTradeTs;
      if (quietPeriod >= this.config.quietThresholdSec) {
        bursts.push(currentBurst);
      }
    }

    if (bursts.length === 0) return ZERO;

    // Use the most recent burst
    const burst = bursts[bursts.length - 1];

    // Compute direction from burst trades
    let yesContracts = 0;
    let noContracts = 0;
    for (const t of burst.trades) {
      if (t.side === "yes") yesContracts += t.count;
      else if (t.side === "no") noContracts += t.count;
    }

    const total = yesContracts + noContracts;
    if (total === 0) return ZERO;

    const dominance = Math.max(yesContracts, noContracts) / total;
    const direction = yesContracts >= noContracts ? 1 : -1;

    // Score: dominance × burst intensity
    const burstDuration = Math.max(burst.endTs - burst.startTs, 1);
    const intensity = Math.min(burst.trades.length / burstDuration, 1); // trades per second, capped
    const score = direction * dominance * intensity;

    // Confidence: more trades in burst = higher confidence
    const confidence = Math.min(burst.trades.length / (this.config.minBurstTrades * 2), 1.0);

    const side = direction > 0 ? "YES" : "NO";
    const reason = `${side} burst: ${burst.trades.length} trades in ${burstDuration}s, ${(dominance * 100).toFixed(0)}% dominance, ${total} contracts`;

    return { name: this.name, score, confidence, reason };
  }

  private findPrevTradeTs(
    allTrades: { ts: number }[],
    firstBurstTrade: { ts: number },
    burstStart: number,
  ): number {
    // Find the trade just before the burst started
    let prevTs = burstStart - this.config.windowSec; // default: window start
    for (const t of allTrades) {
      if (t === firstBurstTrade) break;
      prevTs = t.ts;
    }
    return prevTs;
  }
}
