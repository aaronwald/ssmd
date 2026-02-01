export interface PositionManagerConfig {
  startingBalance: number;
  tradeSize: number;
  minContracts: number;
  maxContracts: number;
  drawdownHaltPercent: number;
  takeProfitCents: number;
  stopLossCents: number;
  timeStopMinutes: number;
  makerFeePerContract: number;
  takerFeePerContract: number;
}

export interface Position {
  model: string;
  ticker: string;
  side: "yes" | "no";
  entryPrice: number;
  contracts: number;
  entryTime: number;
  entryCost: number;
}

export type ExitReason = "take-profit" | "stop-loss" | "time-stop" | "force-close";

export interface ClosedPosition {
  position: Position;
  exitPrice: number;
  exitTime: number;
  reason: ExitReason;
  pnl: number;
  fees: number;
}

export class PositionManager {
  cash: number;
  readonly startingBalance: number;
  openPositions: Position[] = [];
  closedPositions: ClosedPosition[] = [];
  isHalted = false;

  private config: PositionManagerConfig;
  private haltThreshold: number;

  constructor(config: PositionManagerConfig) {
    this.config = config;
    this.cash = config.startingBalance;
    this.startingBalance = config.startingBalance;
    this.haltThreshold = config.startingBalance * (1 - config.drawdownHaltPercent / 100);
  }

  openPosition(
    model: string,
    ticker: string,
    side: "yes" | "no",
    price: number,
    ts: number,
  ): Position | null {
    if (this.isHalted) return null;

    // Check for duplicate (same model + ticker)
    if (this.openPositions.some(p => p.model === model && p.ticker === ticker)) {
      return null;
    }

    // Random contract sizing between minContracts and maxContracts
    const min = this.config.minContracts;
    const max = this.config.maxContracts;
    const contracts = min + Math.floor(Math.random() * (max - min + 1));
    if (contracts <= 0) return null;

    // Entry cost: YES costs price, NO costs (100 - price) per contract + maker fee
    const pricePerContract = side === "yes" ? price : 100 - price;
    const entryCostCents = pricePerContract * contracts + this.config.makerFeePerContract * contracts;
    const entryCostDollars = entryCostCents / 100;

    if (entryCostDollars > this.cash) return null;

    const position: Position = {
      model,
      ticker,
      side: side as "yes" | "no",
      entryPrice: price,
      contracts,
      entryTime: ts,
      entryCost: entryCostDollars,
    };

    this.cash -= entryCostDollars;
    this.openPositions.push(position);
    return position;
  }

  checkExits(
    currentPrice: number,
    closeTs: number,
    forceExitBufferMin: number,
    ticker: string,
    currentTs: number,
  ): ClosedPosition[] {
    const closed: ClosedPosition[] = [];
    const remaining: Position[] = [];

    for (const pos of this.openPositions) {
      if (pos.ticker !== ticker) {
        remaining.push(pos);
        continue;
      }

      const exit = this.evaluateExit(pos, currentPrice, closeTs, forceExitBufferMin, currentTs);
      if (exit) {
        closed.push(exit);
      } else {
        remaining.push(pos);
      }
    }

    this.openPositions = remaining;

    // Credit cash for closed positions
    for (const c of closed) {
      // Exit revenue: YES receives exitPrice, NO receives (100 - exitPrice) per contract
      const pricePerContract = c.position.side === "yes" ? c.exitPrice : 100 - c.exitPrice;
      const exitRevenueCents = pricePerContract * c.position.contracts;
      const exitFeeCents = c.fees * 100;
      const exitRevenueDollars = (exitRevenueCents - exitFeeCents) / 100;
      this.cash += exitRevenueDollars;
      this.closedPositions.push(c);
    }

    // Check drawdown halt using portfolio value (cash + open position value)
    if (this.portfolioValue(currentPrice, ticker) <= this.haltThreshold) {
      this.isHalted = true;
    }

    return closed;
  }

  private evaluateExit(
    pos: Position,
    currentPrice: number,
    closeTs: number,
    forceExitBufferMin: number,
    currentTs: number,
  ): ClosedPosition | null {
    const priceDelta = pos.side === "yes"
      ? currentPrice - pos.entryPrice
      : pos.entryPrice - currentPrice;

    // Force-close near market end
    if (closeTs > 0 && currentTs >= closeTs - forceExitBufferMin * 60) {
      return this.closePosition(pos, currentPrice, currentTs, "force-close", true);
    }

    // Take-profit
    if (priceDelta >= this.config.takeProfitCents) {
      return this.closePosition(pos, currentPrice, currentTs, "take-profit", false);
    }

    // Stop-loss
    if (priceDelta <= -this.config.stopLossCents) {
      return this.closePosition(pos, currentPrice, currentTs, "stop-loss", true);
    }

    // Time-stop
    const elapsedMinutes = (currentTs - pos.entryTime) / 60;
    if (elapsedMinutes >= this.config.timeStopMinutes) {
      return this.closePosition(pos, currentPrice, currentTs, "time-stop", true);
    }

    return null;
  }

  private closePosition(
    pos: Position,
    exitPrice: number,
    exitTime: number,
    reason: ExitReason,
    isTaker: boolean,
  ): ClosedPosition {
    const feePerContract = isTaker
      ? this.config.takerFeePerContract
      : this.config.makerFeePerContract;
    const totalFeeCents = feePerContract * pos.contracts;
    const totalFeeDollars = totalFeeCents / 100;

    const priceDelta = pos.side === "yes"
      ? exitPrice - pos.entryPrice
      : pos.entryPrice - exitPrice;
    const grossPnlDollars = (priceDelta * pos.contracts) / 100;
    const netPnl = grossPnlDollars - totalFeeDollars;

    return {
      position: pos,
      exitPrice,
      exitTime,
      reason,
      pnl: netPnl,
      fees: totalFeeDollars,
    };
  }

  canEnter(closeTs: number, noEntryBufferMin: number, currentTs: number): boolean {
    if (this.isHalted) return false;
    if (closeTs > 0 && currentTs >= closeTs - noEntryBufferMin * 60) return false;
    return true;
  }

  /**
   * Mark-to-market value of open positions at a given price for a ticker.
   * Positions for other tickers use their entry price (last known).
   */
  private portfolioValue(currentPrice: number, ticker: string): number {
    let openValue = 0;
    for (const pos of this.openPositions) {
      const yesPrice = pos.ticker === ticker ? currentPrice : pos.entryPrice;
      const pricePerContract = pos.side === "yes" ? yesPrice : 100 - yesPrice;
      openValue += (pricePerContract * pos.contracts) / 100;
    }
    return this.cash + openValue;
  }

  getSummary(): { balance: number; totalTrades: number; wins: number; losses: number; totalPnl: number; drawdownPercent: number } {
    const wins = this.closedPositions.filter(p => p.pnl > 0).length;
    const losses = this.closedPositions.filter(p => p.pnl <= 0).length;
    const totalPnl = this.closedPositions.reduce((sum, p) => sum + p.pnl, 0);
    const drawdownPercent = ((this.startingBalance - this.cash) / this.startingBalance) * 100;

    return {
      balance: this.cash,
      totalTrades: this.closedPositions.length,
      wins,
      losses,
      totalPnl,
      drawdownPercent: Math.max(0, drawdownPercent),
    };
  }
}
