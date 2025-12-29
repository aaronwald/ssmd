// Volume Signal: Fire when a ticker crosses $1M USD volume in 30 minutes
import type { VolumeProfileState } from "../../src/state/volume_profile.ts";

// Track which tickers have already fired to avoid duplicate fires
const firedTickers = new Set<string>();

export const signal = {
  id: "volume-1m-30min",
  name: "Volume Crosses $1M in 30 Minutes",
  requires: ["volumeProfile"],

  evaluate(state: { volumeProfile: VolumeProfileState }): boolean {
    const { ticker, dollarVolume } = state.volumeProfile;

    // Only fire once per ticker when crossing $1M threshold
    if (dollarVolume >= 1_000_000 && !firedTickers.has(ticker)) {
      firedTickers.add(ticker);
      return true;
    }

    // Reset if volume drops below threshold (allows re-fire on next crossing)
    if (dollarVolume < 1_000_000 && firedTickers.has(ticker)) {
      firedTickers.delete(ticker);
    }

    return false;
  },

  payload(state: { volumeProfile: VolumeProfileState }) {
    return {
      ticker: state.volumeProfile.ticker,
      dollarVolume: state.volumeProfile.dollarVolume,
      contractVolume: state.volumeProfile.totalVolume,
      tradeCount: state.volumeProfile.tradeCount,
      buyRatio: state.volumeProfile.ratio,
      windowMs: state.volumeProfile.windowMs,
      // lastUpdate is in seconds, convert to ms for Date
      lastUpdate: new Date(state.volumeProfile.lastUpdate * 1000).toISOString(),
    };
  },
};
