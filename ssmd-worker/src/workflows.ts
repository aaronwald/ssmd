import { proxyActivities } from '@temporalio/workflow';
import type * as activities from './activities';

const { syncSecmaster, syncSeries, syncFees } = proxyActivities<typeof activities>({
  startToCloseTimeout: '30 minutes',
  heartbeatTimeout: '2 minutes', // Detect dead workers quickly via progress markers
  retry: {
    initialInterval: '1 minute',
    backoffCoefficient: 3,
    maximumAttempts: 3,
    maximumInterval: '15 minutes',
  },
});

// Archiver sync activities - longer timeout for GCS upload
const { syncArchiverGcs } = proxyActivities<typeof activities>({
  startToCloseTimeout: '15 minutes',
  retry: {
    initialInterval: '1 minute',
    backoffCoefficient: 3,
    maximumAttempts: 3,
    maximumInterval: '15 minutes',
  },
});

// Health check activities - moderate timeout
const { runDataQualityCheck } = proxyActivities<typeof activities>({
  startToCloseTimeout: '10 minutes',
  retry: {
    initialInterval: '1 minute',
    backoffCoefficient: 3,
    maximumAttempts: 3,
    maximumInterval: '15 minutes',
  },
});

// Multi-exchange sync activities - shorter timeout, no heartbeat needed
const { syncKraken, syncPolymarket } = proxyActivities<typeof activities>({
  startToCloseTimeout: '10 minutes',
  retry: {
    initialInterval: '1 minute',
    backoffCoefficient: 3,
    maximumAttempts: 3,
    maximumInterval: '15 minutes',
  },
});

// Separate proxy for notifications - short timeout, minimal retries
const { sendNotification } = proxyActivities<typeof activities>({
  startToCloseTimeout: '30 seconds',
  retry: {
    maximumAttempts: 2,
  },
});

export interface SecmasterInput {
  /** If true, only sync active/open records (fast incremental sync) - LEGACY */
  activeOnly?: boolean;
  /** Use series-based sync (faster) instead of time-based */
  bySeries?: boolean;
  /** Filter to specific category (e.g., "Economics", "Sports") */
  category?: string;
  /** Filter to specific tags (e.g., ["Football"]) */
  tags?: string[];
  /** For Sports, only sync game series (GAME/MATCH patterns) */
  gamesOnly?: boolean;
  /** Minimum volume threshold for series (filters low-activity series) */
  minVolume?: number;
  /** Only sync markets closing within N days */
  minCloseDaysAgo?: number;
}

export interface WorkflowResult {
  success: boolean;
  durationMs: number;
  message: string;
}

/**
 * Secmaster workflow - syncs markets and events from Kalshi API to ssmd database
 *
 * Modes:
 * - Series-based (recommended): bySeries=true, optionally with tags for filtering
 *   - Syncs series metadata first, then fetches markets per series
 *   - Much faster than time-based (~30s vs minutes)
 *   - Can be horizontally scaled by running separate jobs per tag
 *
 * - Legacy modes:
 *   - activeOnly=true: Fast incremental sync using time filters (~2 min)
 *   - activeOnly=false: Full sync of all markets (~20 min)
 */
export async function secmasterWorkflow(input: SecmasterInput): Promise<WorkflowResult> {
  const startTime = Date.now();

  // For series-based sync, first sync series metadata
  if (input.bySeries) {
    const seriesResult = await syncSeries(input.category, input.gamesOnly);
    if (!seriesResult.success) {
      const errorMsg = seriesResult.stderr.slice(0, 500);
      try {
        await sendNotification({
          title: 'SSMD Series Sync Failed',
          message: 'Exit code: ' + seriesResult.exitCode + '\n' + errorMsg,
          priority: 'high',
          tags: ['x', 'ssmd'],
        });
      } catch (e) {
        console.error('Failed to send notification:', e);
      }
      throw new Error('Series sync failed: ' + seriesResult.stderr);
    }
  }

  // Then sync markets (by series or legacy mode)
  const result = await syncSecmaster({
    bySeries: input.bySeries,
    category: input.category,
    tags: input.tags,
    activeOnly: input.activeOnly,
    minVolume: input.minVolume,
    minCloseDaysAgo: input.minCloseDaysAgo,
  });

  if (!result.success) {
    const errorMsg = result.stderr.slice(0, 500);
    try {
      await sendNotification({
        title: 'SSMD Secmaster Sync Failed',
        message: 'Exit code: ' + result.exitCode + '\n' + errorMsg,
        priority: 'high',
        tags: ['x', 'ssmd'],
      });
    } catch (e) {
      console.error('Failed to send notification:', e);
    }
    throw new Error('Secmaster sync failed: ' + result.stderr);
  }

  const totalDurationMs = Date.now() - startTime;
  let mode = 'full';
  if (input.bySeries) {
    if (input.tags && input.tags.length > 0) {
      mode = `series-based (tags: ${input.tags.join(', ')})`;
    } else if (input.category) {
      mode = `series-based (${input.category})`;
    } else {
      mode = 'series-based';
    }
  } else if (input.activeOnly) {
    mode = 'incremental';
  }

  return {
    success: true,
    durationMs: totalDurationMs,
    message: `Secmaster ${mode} sync completed in ${Math.round(totalDurationMs / 1000)}s`,
  };
}

/**
 * Fees workflow - syncs fee schedules from Kalshi API to ssmd database
 */
export async function feesWorkflow(): Promise<WorkflowResult> {
  const result = await syncFees();

  if (!result.success) {
    const errorMsg = result.stderr.slice(0, 500);
    try {
      await sendNotification({
        title: 'SSMD Fees Sync Failed',
        message: 'Exit code: ' + result.exitCode + '\n' + errorMsg,
        priority: 'high',
        tags: ['x', 'ssmd'],
      });
    } catch (e) {
      console.error('Failed to send notification:', e);
    }
    throw new Error('Fees sync failed: ' + result.stderr);
  }

  return {
    success: true,
    durationMs: result.durationMs,
    message: 'Fees sync completed in ' + Math.round(result.durationMs / 1000) + 's',
  };
}

export interface ArchiverSyncInput {
  /** Name of the Archiver CR (e.g., "kalshi-archiver") */
  archiverName: string;
}

/**
 * Archiver GCS sync workflow - syncs archiver local data to GCS for durability
 *
 * Runs `ssmd archiver sync <name> --wait` which creates a K8s Job
 * that uses gsutil to rsync local PVC data to GCS bucket.
 */
export async function archiverSyncWorkflow(input: ArchiverSyncInput): Promise<WorkflowResult> {
  const result = await syncArchiverGcs(input.archiverName);

  if (!result.success) {
    const errorMsg = result.stderr.slice(0, 500);
    try {
      await sendNotification({
        title: `SSMD Archiver Sync Failed: ${input.archiverName}`,
        message: 'Exit code: ' + result.exitCode + '\n' + errorMsg,
        priority: 'high',
        tags: ['x', 'ssmd'],
      });
    } catch (e) {
      console.error('Failed to send notification:', e);
    }
    throw new Error(`Archiver sync failed for ${input.archiverName}: ` + result.stderr);
  }

  return {
    success: true,
    durationMs: result.durationMs,
    message: `Archiver sync (${input.archiverName}) completed in ${Math.round(result.durationMs / 1000)}s`,
  };
}

export interface KrakenSyncInput {
  spot?: boolean;
  perps?: boolean;
}

/**
 * Kraken sync workflow - syncs Kraken spot/perps markets to ssmd database
 */
export async function krakenSyncWorkflow(input: KrakenSyncInput): Promise<WorkflowResult> {
  const result = await syncKraken({ spot: input.spot, perps: input.perps });

  if (!result.success) {
    const errorMsg = result.stderr.slice(0, 500);
    try {
      await sendNotification({
        title: 'SSMD Kraken Sync Failed',
        message: 'Exit code: ' + result.exitCode + '\n' + errorMsg,
        priority: 'high',
        tags: ['x', 'ssmd'],
      });
    } catch (e) {
      console.error('Failed to send notification:', e);
    }
    throw new Error('Kraken sync failed: ' + result.stderr);
  }

  return {
    success: true,
    durationMs: result.durationMs,
    message: 'Kraken sync completed in ' + Math.round(result.durationMs / 1000) + 's',
  };
}

/**
 * Polymarket sync workflow - syncs Polymarket markets to ssmd database
 */
export async function polymarketSyncWorkflow(): Promise<WorkflowResult> {
  const result = await syncPolymarket();

  if (!result.success) {
    const errorMsg = result.stderr.slice(0, 500);
    try {
      await sendNotification({
        title: 'SSMD Polymarket Sync Failed',
        message: 'Exit code: ' + result.exitCode + '\n' + errorMsg,
        priority: 'high',
        tags: ['x', 'ssmd'],
      });
    } catch (e) {
      console.error('Failed to send notification:', e);
    }
    throw new Error('Polymarket sync failed: ' + result.stderr);
  }

  return {
    success: true,
    durationMs: result.durationMs,
    message: 'Polymarket sync completed in ' + Math.round(result.durationMs / 1000) + 's',
  };
}

/**
 * Health check workflow - daily scoring of Phase 1 feeds
 *
 * Runs `ssmd health daily --json`, parses the report, and sends a formatted
 * summary to ntfy topic ssmd-data-quality.
 *
 * Schedule: ssmd-data-quality-daily, daily at 06:00 UTC (1:00 AM ET)
 */
export async function dataQualityWorkflow(): Promise<WorkflowResult> {
  const startTime = Date.now();

  const result = await runDataQualityCheck();

  if (!result.success || !result.report) {
    const errorMsg = result.error?.slice(0, 500) || 'Unknown error';
    try {
      await sendNotification({
        title: 'Health Check: FAILED',
        message: 'Health check failed to run\n' + errorMsg,
        priority: 'urgent',
        tags: ['rotating_light', 'ssmd'],
        topic: 'ssmd-data-quality',
      });
    } catch (e) {
      console.error('Failed to send notification:', e);
    }
    throw new Error('Health check failed: ' + (result.error || 'no report'));
  }

  const report = result.report;

  // Format ntfy message
  const feedLines: string[] = [];
  const kc = report.feeds['kalshi-crypto'];
  if (kc) {
    const msgs = kc.messageCount != null ? fmtK(kc.messageCount as number) : '?';
    feedLines.push(`Kalshi Crypto:    ${kc.score}/100 (${msgs} msgs, fresh: ${kc.freshnessScore ?? 0})`);
  }
  const kf = report.feeds['kraken-futures'];
  if (kf) {
    const msgs = kf.messageCount != null ? fmtK(kf.messageCount as number) : '?';
    feedLines.push(`Kraken Futures:   ${kf.score}/100 (${msgs} msgs, fresh: ${kf.freshnessScore ?? 0})`);
  }
  const pm = report.feeds['polymarket'];
  if (pm) {
    const msgs = pm.messageCount != null ? fmtK(pm.messageCount as number) : '?';
    feedLines.push(`Polymarket:       ${pm.score}/100 (${msgs} msgs, fresh: ${pm.freshnessScore ?? 0})`);
  }
  const fr = report.feeds['funding-rate'];
  if (fr) {
    const age = fr.lastFlushAge != null ? fmtAge(fr.lastFlushAge as number) : '?';
    feedLines.push(`Funding Rate:     ${fr.score}/100 (${fr.snapshots ?? 0} snaps, ${fr.products ?? 0} products, ${age})`);
  }
  const as = report.feeds['archive-sync'];
  if (as) {
    const details = as.details as Record<string, { score: number; lastSyncAge: number | null }>;
    if (details) {
      const parts = Object.entries(details)
        .map(([name, d]) => `${name.replace('archiver-', '')}: ${d.lastSyncAge != null ? d.lastSyncAge + 'h' : 'never'}`)
        .join(', ');
      feedLines.push(`GCS Archive:      ${as.score}/100 (${parts})`);
    }
  }

  let body = feedLines.join('\n');
  body += `\n\nComposite: ${report.composite}/100 ${report.grade}`;
  if (report.issues.length > 0) {
    body += '\n\nIssues:\n' + report.issues.map(i => '- ' + i).join('\n');
  }
  // Determine ntfy priority and tags
  let priority: 'default' | 'high' | 'urgent' = 'default';
  let tagEmoji = 'chart_with_upwards_trend';
  if (report.grade === 'YELLOW') {
    priority = 'high';
    tagEmoji = 'warning';
  } else if (report.grade === 'RED') {
    priority = 'urgent';
    tagEmoji = 'rotating_light';
  }

  try {
    await sendNotification({
      title: `Health: ${report.grade} (${report.composite}/100) - ${report.date}`,
      message: body,
      priority,
      tags: [tagEmoji, 'ssmd'],
      topic: 'ssmd-data-quality',
    });
  } catch (e) {
    console.error('Failed to send health notification:', e);
  }

  const totalDurationMs = Date.now() - startTime;
  return {
    success: true,
    durationMs: totalDurationMs,
    message: `Health daily: ${report.grade} (${report.composite}/100) in ${Math.round(totalDurationMs / 1000)}s`,
  };
}

function fmtK(n: number): string {
  if (n >= 1000) return (n / 1000).toFixed(1) + 'K';
  return String(n);
}

function fmtAge(seconds: number): string {
  if (seconds < 60) return seconds + 's ago';
  if (seconds < 3600) return Math.round(seconds / 60) + 'm ago';
  return (seconds / 3600).toFixed(1) + 'h ago';
}
