import {
  connect,
  StringCodec,
  Events,
} from "npm:nats";
import type { MomentumConfig } from "./config.ts";
import { MarketState } from "./market-state.ts";
import { PositionManager } from "./position-manager.ts";
import { Reporter } from "./reporter.ts";
import { VolumeSpikeMomentum } from "./models/volume-spike.ts";
import { TradeFlowImbalance } from "./models/trade-flow.ts";
import { PriceAcceleration } from "./models/price-acceleration.ts";
import type { MomentumModel } from "./models/types.ts";
import { parseMomentumRecord } from "./parse.ts";

const sc = StringCodec();

export async function runMomentum(config: MomentumConfig): Promise<void> {
  const models: MomentumModel[] = [];
  if (config.models.volumeSpike.enabled) {
    models.push(new VolumeSpikeMomentum(config.models.volumeSpike));
  }
  if (config.models.tradeFlow.enabled) {
    models.push(new TradeFlowImbalance(config.models.tradeFlow));
  }
  if (config.models.priceAcceleration.enabled) {
    models.push(new PriceAcceleration(config.models.priceAcceleration));
  }

  if (models.length === 0) {
    console.error("No models enabled");
    Deno.exit(1);
  }

  const pm = new PositionManager({
    startingBalance: config.portfolio.startingBalance,
    tradeSize: config.portfolio.tradeSize,
    drawdownHaltPercent: config.portfolio.drawdownHaltPercent,
    takeProfitCents: config.positions.takeProfitCents,
    stopLossCents: config.positions.stopLossCents,
    timeStopMinutes: config.positions.timeStopMinutes,
    makerFeePerContract: config.fees.defaultMakerPerContract,
    takerFeePerContract: config.fees.defaultTakerPerContract,
  });

  const reporter = new Reporter(config.reporting.summaryIntervalMinutes);
  const marketStates = new Map<string, MarketState>();
  const activatedTickers = new Set<string>();

  const activationThreshold = config.activation.dollarVolume;
  const activationWindowSec = config.activation.windowMinutes * 60;

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
  const consumer = await js.consumers.get(config.nats.stream);
  const messages = await consumer.consume();

  console.log(`[momentum] Connected to NATS: ${config.nats.url}`);
  console.log(`[momentum] Stream: ${config.nats.stream}, Filter: ${config.nats.filter ?? "all"}`);
  console.log(`[momentum] Models: ${models.map(m => m.name).join(", ")}`);
  console.log(`[momentum] Portfolio: $${config.portfolio.startingBalance} balance, $${config.portfolio.tradeSize}/trade, ${config.portfolio.drawdownHaltPercent}% drawdown halt`);
  console.log(`[momentum] Activation: $${activationThreshold} in ${config.activation.windowMinutes}min`);
  console.log(``);

  let recordCount = 0;

  for await (const msg of messages) {
    try {
      const raw = JSON.parse(sc.decode(msg.data));
      const record = parseMomentumRecord(raw);
      if (!record || !record.ticker) {
        msg.ack();
        continue;
      }

      recordCount++;

      let state = marketStates.get(record.ticker);
      if (!state) {
        state = new MarketState(record.ticker);
        marketStates.set(record.ticker, state);
      }

      state.update(record);

      // Check activation
      if (!activatedTickers.has(record.ticker)) {
        if (state.isActivated(activationThreshold, activationWindowSec)) {
          activatedTickers.add(record.ticker);
          reporter.logActivation(record.ticker, record.ts);
        } else {
          msg.ack();
          continue;
        }
      }

      // Check exits for this ticker
      const closeTs = state.closeTs ?? 0;
      const exits = pm.checkExits(
        state.lastPrice,
        closeTs,
        config.marketClose.forceExitBufferMinutes,
        record.ticker,
        record.ts,
      );
      for (const exit of exits) {
        reporter.logExit(exit);
      }

      // Check if halted
      if (pm.isHalted) {
        if (exits.length > 0) reporter.logHalt(pm);
        msg.ack();
        continue;
      }

      // Evaluate models for entry signals (only if ticker is still active)
      if (state.isActivated(activationThreshold, activationWindowSec)) {
        if (pm.canEnter(closeTs, config.marketClose.noEntryBufferMinutes, record.ts)) {
          for (const model of models) {
            const signal = model.evaluate(state);
            if (signal) {
              const pos = pm.openPosition(signal.model, signal.ticker, signal.side, signal.price, record.ts);
              if (pos) {
                reporter.logEntry(signal, pos);
              }
            }
          }
        }
      }

      // Periodic summary
      reporter.maybePrintSummary(pm, record.ts);

    } catch (e) {
      if (recordCount <= 10) {
        console.error(`[momentum] Parse error: ${e}`);
      }
    }

    msg.ack();
  }

  console.log(`\n[momentum] Shutting down. ${recordCount.toLocaleString()} records processed.`);
  reporter.printSummary(pm);

  await nc.drain();
  await nc.close();
}
