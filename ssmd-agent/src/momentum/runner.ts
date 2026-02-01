import {
  connect,
  StringCodec,
  Events,
  AckPolicy,
  DeliverPolicy,
} from "npm:nats";
import type { MomentumConfig } from "./config.ts";
import { MarketState } from "./market-state.ts";
import { PositionManager } from "./position-manager.ts";
import { Reporter } from "./reporter.ts";
import { SpreadTightening } from "./signals/spread-tightening.ts";
import { VolumeOnset } from "./signals/volume-onset.ts";
import { MeanReversion } from "./signals/mean-reversion.ts";
import { VolatilitySqueeze } from "./signals/volatility-squeeze.ts";
import { PriceMomentum } from "./signals/price-momentum.ts";
import { TradeImbalance } from "./signals/trade-imbalance.ts";
import { TradeConcentration } from "./signals/trade-concentration.ts";
import { FlowAsymmetry } from "./signals/flow-asymmetry.ts";
import { SpreadVelocity } from "./signals/spread-velocity.ts";
import { VolumePriceDivergence } from "./signals/volume-price-divergence.ts";
import { Composer } from "./signals/composer.ts";
import type { Signal } from "./signals/types.ts";
import { parseMomentumRecord } from "./parse.ts";
import type { MarketRecord } from "../state/types.ts";

const sc = StringCodec();

export interface MomentumState {
  signals: Signal[];
  composer: Composer;
  pm: PositionManager;
  reporter: Reporter;
  marketStates: Map<string, MarketState>;
  activatedTickers: Set<string>;
  tickerCooldowns: Map<string, number>;
  config: MomentumConfig;
  activationThreshold: number;
  activationWindowSec: number;
  recordCount: number;
  debug: boolean;
  debugLogCount: number;
}

export function createMomentumState(config: MomentumConfig): MomentumState {
  const signals: Signal[] = [];
  const weights: number[] = [];

  if (config.signals.spreadTightening.enabled) {
    signals.push(new SpreadTightening({
      spreadWindowMinutes: config.signals.spreadTightening.spreadWindowMinutes,
      narrowingThreshold: config.signals.spreadTightening.narrowingThreshold,
      weight: config.signals.spreadTightening.weight,
    }));
    weights.push(config.signals.spreadTightening.weight);
  }
  if (config.signals.volumeOnset.enabled) {
    signals.push(new VolumeOnset({
      recentWindowSec: config.signals.volumeOnset.recentWindowSec,
      baselineWindowMinutes: config.signals.volumeOnset.baselineWindowMinutes,
      onsetMultiplier: config.signals.volumeOnset.onsetMultiplier,
      weight: config.signals.volumeOnset.weight,
    }));
    weights.push(config.signals.volumeOnset.weight);
  }
  if (config.signals.meanReversion.enabled) {
    signals.push(new MeanReversion({
      anchorWindowMinutes: config.signals.meanReversion.anchorWindowMinutes,
      deviationThresholdCents: config.signals.meanReversion.deviationThresholdCents,
      maxDeviationCents: config.signals.meanReversion.maxDeviationCents,
      recentWindowSec: config.signals.meanReversion.recentWindowSec,
      stallWindowSec: config.signals.meanReversion.stallWindowSec,
      minRecentChangeCents: config.signals.meanReversion.minRecentChangeCents,
      minTrades: config.signals.meanReversion.minTrades,
      weight: config.signals.meanReversion.weight,
    }));
    weights.push(config.signals.meanReversion.weight);
  }
  if (config.signals.volatilitySqueeze.enabled) {
    signals.push(new VolatilitySqueeze({
      squeezeWindowMinutes: config.signals.volatilitySqueeze.squeezeWindowMinutes,
      compressionThreshold: config.signals.volatilitySqueeze.compressionThreshold,
      expansionThreshold: config.signals.volatilitySqueeze.expansionThreshold,
      minBaselineStdDev: config.signals.volatilitySqueeze.minBaselineStdDev,
      maxExpansionRatio: config.signals.volatilitySqueeze.maxExpansionRatio,
      minSnapshots: config.signals.volatilitySqueeze.minSnapshots,
      weight: config.signals.volatilitySqueeze.weight,
    }));
    weights.push(config.signals.volatilitySqueeze.weight);
  }
  if (config.signals.priceMomentum.enabled) {
    signals.push(new PriceMomentum({
      shortWindowSec: config.signals.priceMomentum.shortWindowSec,
      midWindowSec: config.signals.priceMomentum.midWindowSec,
      longWindowSec: config.signals.priceMomentum.longWindowSec,
      minTotalMoveCents: config.signals.priceMomentum.minTotalMoveCents,
      maxAccelRatio: config.signals.priceMomentum.maxAccelRatio,
      minEntryPrice: config.signals.priceMomentum.minEntryPrice,
      maxEntryPrice: config.signals.priceMomentum.maxEntryPrice,
      minTrades: config.signals.priceMomentum.minTrades,
      weight: config.signals.priceMomentum.weight,
    }));
    weights.push(config.signals.priceMomentum.weight);
  }
  if (config.signals.tradeImbalance.enabled) {
    signals.push(new TradeImbalance({
      windowSec: config.signals.tradeImbalance.windowSec,
      minTrades: config.signals.tradeImbalance.minTrades,
      imbalanceThreshold: config.signals.tradeImbalance.imbalanceThreshold,
      sustainedWindowSec: config.signals.tradeImbalance.sustainedWindowSec,
      sustainedThreshold: config.signals.tradeImbalance.sustainedThreshold,
      weight: config.signals.tradeImbalance.weight,
    }));
    weights.push(config.signals.tradeImbalance.weight);
  }
  if (config.signals.tradeConcentration.enabled) {
    signals.push(new TradeConcentration({
      windowSec: config.signals.tradeConcentration.windowSec,
      minTrades: config.signals.tradeConcentration.minTrades,
      concentrationThreshold: config.signals.tradeConcentration.concentrationThreshold,
      weight: config.signals.tradeConcentration.weight,
    }));
    weights.push(config.signals.tradeConcentration.weight);
  }
  if (config.signals.flowAsymmetry.enabled) {
    signals.push(new FlowAsymmetry({
      windowSec: config.signals.flowAsymmetry.windowSec,
      minTrades: config.signals.flowAsymmetry.minTrades,
      asymmetryThreshold: config.signals.flowAsymmetry.asymmetryThreshold,
      weight: config.signals.flowAsymmetry.weight,
    }));
    weights.push(config.signals.flowAsymmetry.weight);
  }
  if (config.signals.spreadVelocity.enabled) {
    signals.push(new SpreadVelocity({
      windowSec: config.signals.spreadVelocity.windowSec,
      minSnapshots: config.signals.spreadVelocity.minSnapshots,
      velocityThreshold: config.signals.spreadVelocity.velocityThreshold,
      weight: config.signals.spreadVelocity.weight,
    }));
    weights.push(config.signals.spreadVelocity.weight);
  }
  if (config.signals.volumePriceDivergence.enabled) {
    signals.push(new VolumePriceDivergence({
      windowSec: config.signals.volumePriceDivergence.windowSec,
      baselineWindowSec: config.signals.volumePriceDivergence.baselineWindowSec,
      volumeMultiplier: config.signals.volumePriceDivergence.volumeMultiplier,
      maxPriceMoveCents: config.signals.volumePriceDivergence.maxPriceMoveCents,
      minTrades: config.signals.volumePriceDivergence.minTrades,
      weight: config.signals.volumePriceDivergence.weight,
    }));
    weights.push(config.signals.volumePriceDivergence.weight);
  }

  if (signals.length === 0) {
    console.error("No signals enabled");
    Deno.exit(1);
  }

  const composer = new Composer(signals, weights, {
    entryThreshold: config.composer.entryThreshold,
    minSignals: config.composer.minSignals,
    maxSlippageCents: config.composer.maxSlippageCents,
  });

  const pm = new PositionManager({
    startingBalance: config.portfolio.startingBalance,
    tradeSize: config.portfolio.tradeSize,
    minContracts: config.portfolio.minContracts,
    maxContracts: config.portfolio.maxContracts,
    drawdownHaltPercent: config.portfolio.drawdownHaltPercent,
    takeProfitCents: config.positions.takeProfitCents,
    stopLossCents: config.positions.stopLossCents,
    timeStopMinutes: config.positions.timeStopMinutes,
    makerFeePerContract: config.fees.defaultMakerPerContract,
    takerFeePerContract: config.fees.defaultTakerPerContract,
  });

  const reporter = new Reporter(config.reporting.summaryIntervalMinutes);

  return {
    signals,
    composer,
    pm,
    reporter,
    marketStates: new Map<string, MarketState>(),
    activatedTickers: new Set<string>(),
    tickerCooldowns: new Map<string, number>(),
    config,
    activationThreshold: config.activation.dollarVolume,
    activationWindowSec: config.activation.windowMinutes * 60,
    recordCount: 0,
    debug: config.reporting.debug,
    debugLogCount: 0,
  };
}

export function processRecord(record: MarketRecord, state: MomentumState): boolean {
  if (!record || !record.ticker) return false;

  state.recordCount++;

  let ms = state.marketStates.get(record.ticker);
  if (!ms) {
    ms = new MarketState(record.ticker);
    state.marketStates.set(record.ticker, ms);
  }

  ms.update(record);

  // Check activation
  if (!state.activatedTickers.has(record.ticker)) {
    if (ms.isActivated(state.activationThreshold, state.activationWindowSec)) {
      state.activatedTickers.add(record.ticker);
      state.reporter.logActivation(record.ticker, record.ts);
    } else {
      return true;
    }
  }

  // Check exits for this ticker
  const closeTs = ms.closeTs ?? 0;
  const exits = state.pm.checkExits(
    ms.lastPrice,
    closeTs,
    state.config.marketClose.forceExitBufferMinutes,
    record.ticker,
    record.ts,
  );
  for (const exit of exits) {
    state.reporter.logExit(exit);
    const cooldownSec = state.config.positions.cooldownSeconds;
    state.tickerCooldowns.set(record.ticker, record.ts + cooldownSec);
  }

  // Check if halted
  if (state.pm.isHalted) {
    if (exits.length > 0) state.reporter.logHalt(state.pm);
    return true;
  }

  // Evaluate signals for entry (only if ticker is still active)
  if (ms.isActivated(state.activationThreshold, state.activationWindowSec)) {
    const cooldownUntil = state.tickerCooldowns.get(record.ticker) ?? 0;
    if (record.ts < cooldownUntil) return true;

    if (state.pm.canEnter(closeTs, state.config.marketClose.noEntryBufferMinutes, record.ts)) {
      const decision = state.composer.evaluate(ms);

      // Diagnostic logging: show signal evaluations for activated tickers
      if (state.debug && decision.signals.length > 0) {
        state.debugLogCount++;
        if (state.debugLogCount <= 500) {
          const time = new Date(record.ts * 1000).toISOString();
          const sigs = decision.signals.map(s => `${s.name}(score=${s.score.toFixed(3)},conf=${s.confidence.toFixed(3)})`).join(" ");
          console.log(`[DEBUG] ${time} ticker=${record.ticker} enter=${decision.enter} score=${decision.score.toFixed(3)} side=${decision.side} price=${decision.price} | ${sigs}`);
        }
      }

      if (decision.enter) {
        const { takeProfitCents, minPriceCents, maxPriceCents } = state.config.positions;

        // Skip entries outside the price band
        if (decision.price < minPriceCents || decision.price > maxPriceCents) return true;

        // Skip entries where max possible gain < take-profit target
        const maxGain = decision.side === "yes"
          ? 100 - decision.price
          : decision.price;
        if (maxGain < takeProfitCents) return true;

        const modelName = decision.signals.map(s => s.name).join("+");
        const pos = state.pm.openPosition(modelName, record.ticker, decision.side, decision.price, record.ts);
        if (pos) {
          state.reporter.logEntry(decision.signals, pos);
        }
      }
    }
  }

  // Periodic summary
  state.reporter.maybePrintSummary(state.pm, record.ts);

  return true;
}

export async function runMomentum(config: MomentumConfig): Promise<void> {
  const state = createMomentumState(config);

  const nc = await connect({
    servers: config.nats.url,
    reconnect: true,
    maxReconnectAttempts: -1,
    reconnectTimeWait: 2000,
    pingInterval: 30000,
    maxPingOut: 3,
  });

  (async () => {
    for await (const status of nc.status()) {
      switch (status.type) {
        case Events.Disconnect:
          console.warn("[momentum] NATS disconnected");
          break;
        case Events.Reconnect:
          console.log(`[momentum] NATS reconnected to ${status.data}`);
          break;
        case Events.Error:
          console.error(`[momentum] NATS error: ${status.data}`);
          break;
      }
    }
  })();

  const js = nc.jetstream();
  const jsm = await nc.jetstreamManager();

  const consumerName = "ssmd-momentum";
  try {
    await jsm.consumers.add(config.nats.stream, {
      durable_name: consumerName,
      filter_subject: config.nats.filter ?? undefined,
      ack_policy: AckPolicy.Explicit,
      deliver_policy: DeliverPolicy.New,
    });
  } catch (e) {
    if (!String(e).includes("already exists")) {
      throw e;
    }
  }

  const consumer = await js.consumers.get(config.nats.stream, consumerName);
  const messages = await consumer.consume();

  console.log(`[momentum] Connected to NATS: ${config.nats.url}`);
  console.log(`[momentum] Stream: ${config.nats.stream}, Filter: ${config.nats.filter ?? "all"}`);
  console.log(`[momentum] Signals: ${state.signals.map(s => s.name).join(", ")}`);
  console.log(`[momentum] Composer: threshold=${config.composer.entryThreshold}, minSignals=${config.composer.minSignals}`);
  console.log(`[momentum] Portfolio: $${config.portfolio.startingBalance} balance, ${config.portfolio.minContracts}-${config.portfolio.maxContracts} contracts/trade, ${config.portfolio.drawdownHaltPercent}% drawdown halt`);
  console.log(`[momentum] Activation: $${state.activationThreshold} in ${config.activation.windowMinutes}min`);
  console.log(``);

  for await (const msg of messages) {
    try {
      const raw = JSON.parse(sc.decode(msg.data));
      const record = parseMomentumRecord(raw);
      if (record) {
        processRecord(record, state);
      }
    } catch (e) {
      if (state.recordCount <= 10) {
        console.error(`[momentum] Parse error: ${e}`);
      }
    }

    msg.ack();
  }

  console.log(`\n[momentum] Shutting down. ${state.recordCount.toLocaleString()} records processed.`);
  state.reporter.printSummary(state.pm);

  await nc.drain();
  await nc.close();
}
