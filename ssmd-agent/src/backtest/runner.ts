// ssmd-agent/src/backtest/runner.ts
import type { OrderBookState } from "../state/orderbook.ts";

export interface BacktestResult {
  fires: number;
  errors: string[];
  fireTimes: string[];
  samplePayloads: unknown[];
  recordsProcessed: number;
  durationMs: number;
}

export interface Signal {
  id: string;
  name?: string;
  requires: string[];
  evaluate(state: { orderbook: OrderBookState }): boolean;
  payload(state: { orderbook: OrderBookState }): unknown;
}

export async function compileSignal(code: string): Promise<Signal> {
  // Use data URL for dynamic import in sandbox
  const wrappedCode = `
    ${code}
    export default signal;
  `;
  const dataUrl = `data:text/typescript;base64,${btoa(wrappedCode)}`;

  try {
    const module = await import(dataUrl);
    return module.default as Signal;
  } catch (e) {
    throw new Error(`Signal compilation failed: ${e}`);
  }
}

export async function runBacktest(
  signalCode: string,
  states: OrderBookState[]
): Promise<BacktestResult> {
  const start = Date.now();
  const errors: string[] = [];
  const fires: { time: string; payload: unknown }[] = [];

  let signal: Signal;
  try {
    signal = await compileSignal(signalCode);
  } catch (e) {
    return {
      fires: 0,
      errors: [(e as Error).message],
      fireTimes: [],
      samplePayloads: [],
      recordsProcessed: 0,
      durationMs: Date.now() - start,
    };
  }

  for (const state of states) {
    try {
      const stateMap = { orderbook: state };
      if (signal.evaluate(stateMap)) {
        fires.push({
          time: new Date(state.lastUpdate).toISOString(),
          payload: signal.payload(stateMap),
        });
      }
    } catch (e) {
      errors.push((e as Error).message);
      if (errors.length >= 10) break; // Limit errors
    }
  }

  return {
    fires: fires.length,
    errors,
    fireTimes: fires.slice(0, 20).map((f) => f.time),
    samplePayloads: fires.slice(0, 5).map((f) => f.payload),
    recordsProcessed: states.length,
    durationMs: Date.now() - start,
  };
}
