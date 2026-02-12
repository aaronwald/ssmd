import { spawn } from 'child_process';
import { createInterface } from 'readline';
import { createTransport } from 'nodemailer';
import { Context } from '@temporalio/activity';

export interface SyncResult {
  success: boolean;
  stdout: string;
  stderr: string;
  exitCode: number;
  durationMs: number;
}

export interface SecmasterSyncOptions {
  /** Use series-based sync (faster) instead of time-based */
  bySeries?: boolean;
  /** Filter to specific category (e.g., "Economics", "Sports") */
  category?: string;
  /** Filter to specific tags (e.g., "Football") */
  tags?: string[];
  /** For Sports, only sync game series (GAME/MATCH patterns) */
  gamesOnly?: boolean;
  /** If true, only sync active/open records (legacy mode) */
  activeOnly?: boolean;
  /** Minimum volume threshold for series (filters low-activity series) */
  minVolume?: number;
  /** Only sync markets closing within N days (converts to --min-close-days-ago) */
  minCloseDaysAgo?: number;
}

/**
 * Execute ssmd series sync command
 * Syncs series metadata from Kalshi to ssmd database
 * @param category - Optional category to sync (e.g., "Economics", "Sports")
 * @param gamesOnly - If true, only sync game series for Sports
 */
export async function syncSeries(category?: string, gamesOnly?: boolean): Promise<SyncResult> {
  const startTime = Date.now();
  const command = process.env.SSMD_COMMAND || 'ssmd';
  const args = ['series', 'sync'];

  if (category) {
    args.push(`--category=${category}`);
  }

  if (gamesOnly) {
    args.push('--games-only');
  }

  const databaseUrl = process.env.DATABASE_URL;
  if (!databaseUrl) {
    return {
      success: false,
      stdout: '',
      stderr: 'DATABASE_URL environment variable not set',
      exitCode: 1,
      durationMs: Date.now() - startTime,
    };
  }

  const categoryInfo = category ? ` for category: ${category}` : '';
  console.log(`Starting ssmd series sync${categoryInfo}`);

  return new Promise((resolve) => {
    let stdout = '';
    let stderr = '';
    const child = spawn(command, args, {
      env: {
        ...process.env,
        TRIGGERED_BY: 'temporal',
      },
    });

    child.stdout.on('data', (data) => {
      stdout += data.toString();
    });

    child.stderr.on('data', (data) => {
      stderr += data.toString();
    });

    child.on('close', (code) => {
      const exitCode = code ?? 1;
      resolve({
        success: exitCode === 0,
        stdout,
        stderr,
        exitCode,
        durationMs: Date.now() - startTime,
      });
    });

    child.on('error', (err) => {
      resolve({
        success: false,
        stdout,
        stderr: stderr + '\n' + err.message,
        exitCode: 1,
        durationMs: Date.now() - startTime,
      });
    });

    const timeout = setTimeout(() => {
      child.kill('SIGTERM');
      resolve({
        success: false,
        stdout,
        stderr: stderr + '\nProcess killed: timeout exceeded',
        exitCode: 124,
        durationMs: Date.now() - startTime,
      });
    }, 5 * 60 * 1000);

    child.on('close', () => clearTimeout(timeout));
  });
}

/**
 * Execute ssmd secmaster sync command with streaming output and heartbeats
 * Syncs Kalshi events/markets to ssmd database
 *
 * Parses PROGRESS: markers from CLI to:
 * - Heartbeat to Temporal on each series (detect dead workers in ~2 min)
 * - Track consecutive errors for fail-fast (CLI handles this, but we log it)
 *
 * @param options - Sync options (bySeries, tags, gamesOnly, activeOnly)
 */
export async function syncSecmaster(options: SecmasterSyncOptions | boolean = {}): Promise<SyncResult> {
  const startTime = Date.now();
  const command = process.env.SSMD_COMMAND || 'ssmd';

  // Handle legacy boolean signature for backwards compatibility
  const opts: SecmasterSyncOptions = typeof options === 'boolean'
    ? { activeOnly: options }
    : options;

  const args = ['secmaster', 'sync'];

  if (opts.bySeries) {
    args.push('--by-series');

    if (opts.category) {
      args.push(`--category=${opts.category}`);
    }

    if (opts.tags && opts.tags.length > 0) {
      for (const tag of opts.tags) {
        args.push(`--tag=${tag}`);
      }
    }

    if (opts.minVolume) {
      args.push(`--min-volume=${opts.minVolume}`);
    }

    if (opts.minCloseDaysAgo) {
      args.push(`--min-close-days-ago=${opts.minCloseDaysAgo}`);
    }
  } else if (opts.activeOnly) {
    args.push('--active-only');
  }

  const databaseUrl = process.env.DATABASE_URL;
  if (!databaseUrl) {
    return {
      success: false,
      stdout: '',
      stderr: 'DATABASE_URL environment variable not set',
      exitCode: 1,
      durationMs: Date.now() - startTime,
    };
  }

  let mode = 'full';
  if (opts.bySeries) {
    const volInfo = opts.minVolume ? `, minVolume=${opts.minVolume}` : '';
    if (opts.tags && opts.tags.length > 0) {
      mode = `series-based (tags: ${opts.tags.join(', ')}${volInfo})`;
    } else if (opts.category) {
      mode = `series-based (category: ${opts.category}${volInfo})`;
    } else {
      mode = `series-based${volInfo}`;
    }
  } else if (opts.activeOnly) {
    mode = 'incremental (active only)';
  }
  console.log(`Starting ssmd secmaster sync (${mode})`);

  return new Promise((resolve) => {
    let stdout = '';
    let stderr = '';
    let lastProgress = '';

    const child = spawn(command, args, {
      env: {
        ...process.env,
        TRIGGERED_BY: 'temporal',
      },
    });

    // Parse stdout line by line for progress markers
    const rl = createInterface({ input: child.stdout });
    rl.on('line', (line) => {
      stdout += line + '\n';

      // Parse progress markers and heartbeat
      if (line.startsWith('PROGRESS:')) {
        const parts = line.split(':');
        const type = parts[1];

        if (type === 'series') {
          // PROGRESS:series:50/1521:KXTICKER
          lastProgress = parts.slice(2).join(':');
          try {
            Context.current().heartbeat({ progress: lastProgress });
          } catch (e) {
            // Heartbeat may fail if activity is being cancelled
            console.log(`Heartbeat failed: ${e}`);
          }
        } else if (type === 'error') {
          // PROGRESS:error:KXTICKER:error message
          console.log(`Series error: ${parts.slice(2).join(':')}`);
        } else if (type === 'fatal') {
          // PROGRESS:fatal:message
          console.log(`Fatal error: ${parts.slice(2).join(':')}`);
        } else if (type === 'complete') {
          // PROGRESS:complete:events=X,markets=Y,errors=Z
          console.log(`Sync complete: ${parts.slice(2).join(':')}`);
        }
      }
    });

    // Capture stderr
    child.stderr.on('data', (data) => {
      stderr += data.toString();
    });

    // Handle process exit
    child.on('close', (code) => {
      const exitCode = code ?? 1;
      resolve({
        success: exitCode === 0,
        stdout,
        stderr,
        exitCode,
        durationMs: Date.now() - startTime,
      });
    });

    child.on('error', (err) => {
      resolve({
        success: false,
        stdout,
        stderr: stderr + '\n' + err.message,
        exitCode: 1,
        durationMs: Date.now() - startTime,
      });
    });

    // Set a 30 minute timeout
    const timeout = setTimeout(() => {
      child.kill('SIGTERM');
      resolve({
        success: false,
        stdout,
        stderr: stderr + '\nProcess killed: 30 minute timeout exceeded',
        exitCode: 124,
        durationMs: Date.now() - startTime,
      });
    }, 30 * 60 * 1000);

    child.on('close', () => clearTimeout(timeout));
  });
}

/**
 * Execute ssmd fees sync command
 * Syncs Kalshi fee schedules to ssmd database
 */
export async function syncFees(): Promise<SyncResult> {
  const startTime = Date.now();
  const command = process.env.SSMD_COMMAND || 'ssmd';
  const args = ['fees', 'sync'];

  const databaseUrl = process.env.DATABASE_URL;
  if (!databaseUrl) {
    return {
      success: false,
      stdout: '',
      stderr: 'DATABASE_URL environment variable not set',
      exitCode: 1,
      durationMs: Date.now() - startTime,
    };
  }

  console.log('Starting ssmd fees sync');

  return new Promise((resolve) => {
    let stdout = '';
    let stderr = '';
    const child = spawn(command, args, {
      env: {
        ...process.env,
        TRIGGERED_BY: 'temporal',
      },
    });

    child.stdout.on('data', (data) => {
      stdout += data.toString();
    });

    child.stderr.on('data', (data) => {
      stderr += data.toString();
    });

    child.on('close', (code) => {
      const exitCode = code ?? 1;
      resolve({
        success: exitCode === 0,
        stdout,
        stderr,
        exitCode,
        durationMs: Date.now() - startTime,
      });
    });

    child.on('error', (err) => {
      resolve({
        success: false,
        stdout,
        stderr: stderr + '\n' + err.message,
        exitCode: 1,
        durationMs: Date.now() - startTime,
      });
    });

    const timeout = setTimeout(() => {
      child.kill('SIGTERM');
      resolve({
        success: false,
        stdout,
        stderr: stderr + '\nProcess killed: timeout exceeded',
        exitCode: 124,
        durationMs: Date.now() - startTime,
      });
    }, 5 * 60 * 1000);

    child.on('close', () => clearTimeout(timeout));
  });
}

/**
 * Execute ssmd kraken sync command
 * Syncs Kraken spot and/or perps markets to ssmd database
 */
export async function syncKraken(options: { spot?: boolean; perps?: boolean }): Promise<SyncResult> {
  const startTime = Date.now();
  const command = process.env.SSMD_COMMAND || 'ssmd';
  const args = ['kraken', 'sync'];

  if (options.spot) {
    args.push('--spot');
  }
  if (options.perps) {
    args.push('--perps');
  }

  const databaseUrl = process.env.DATABASE_URL;
  if (!databaseUrl) {
    return {
      success: false,
      stdout: '',
      stderr: 'DATABASE_URL environment variable not set',
      exitCode: 1,
      durationMs: Date.now() - startTime,
    };
  }

  const filters = [options.spot && 'spot', options.perps && 'perps'].filter(Boolean).join('+') || 'all';
  console.log(`Starting ssmd kraken sync (${filters})`);

  return new Promise((resolve) => {
    let stdout = '';
    let stderr = '';
    const child = spawn(command, args, {
      env: {
        ...process.env,
        TRIGGERED_BY: 'temporal',
      },
    });

    child.stdout.on('data', (data) => {
      stdout += data.toString();
    });

    child.stderr.on('data', (data) => {
      stderr += data.toString();
    });

    child.on('close', (code) => {
      const exitCode = code ?? 1;
      resolve({
        success: exitCode === 0,
        stdout,
        stderr,
        exitCode,
        durationMs: Date.now() - startTime,
      });
    });

    child.on('error', (err) => {
      resolve({
        success: false,
        stdout,
        stderr: stderr + '\n' + err.message,
        exitCode: 1,
        durationMs: Date.now() - startTime,
      });
    });

    const timeout = setTimeout(() => {
      child.kill('SIGTERM');
      resolve({
        success: false,
        stdout,
        stderr: stderr + '\nProcess killed: timeout exceeded',
        exitCode: 124,
        durationMs: Date.now() - startTime,
      });
    }, 5 * 60 * 1000);

    child.on('close', () => clearTimeout(timeout));
  });
}

/**
 * Execute ssmd polymarket sync command
 * Syncs Polymarket markets to ssmd database
 */
export async function syncPolymarket(): Promise<SyncResult> {
  const startTime = Date.now();
  const command = process.env.SSMD_COMMAND || 'ssmd';
  const args = ['polymarket', 'sync'];

  const databaseUrl = process.env.DATABASE_URL;
  if (!databaseUrl) {
    return {
      success: false,
      stdout: '',
      stderr: 'DATABASE_URL environment variable not set',
      exitCode: 1,
      durationMs: Date.now() - startTime,
    };
  }

  console.log('Starting ssmd polymarket sync');

  return new Promise((resolve) => {
    let stdout = '';
    let stderr = '';
    const child = spawn(command, args, {
      env: {
        ...process.env,
        TRIGGERED_BY: 'temporal',
      },
    });

    child.stdout.on('data', (data) => {
      stdout += data.toString();
    });

    child.stderr.on('data', (data) => {
      stderr += data.toString();
    });

    child.on('close', (code) => {
      const exitCode = code ?? 1;
      resolve({
        success: exitCode === 0,
        stdout,
        stderr,
        exitCode,
        durationMs: Date.now() - startTime,
      });
    });

    child.on('error', (err) => {
      resolve({
        success: false,
        stdout,
        stderr: stderr + '\n' + err.message,
        exitCode: 1,
        durationMs: Date.now() - startTime,
      });
    });

    const timeout = setTimeout(() => {
      child.kill('SIGTERM');
      resolve({
        success: false,
        stdout,
        stderr: stderr + '\nProcess killed: timeout exceeded',
        exitCode: 124,
        durationMs: Date.now() - startTime,
      });
    }, 5 * 60 * 1000);

    child.on('close', () => clearTimeout(timeout));
  });
}

/**
 * Execute ssmd archiver sync command
 * Triggers GCS sync for a named archiver via K8s Job (gsutil rsync)
 * @param archiverName - Name of the Archiver CR (e.g., "kalshi-archiver")
 */
export async function syncArchiverGcs(archiverName: string): Promise<SyncResult> {
  const startTime = Date.now();
  const command = process.env.SSMD_COMMAND || 'ssmd';
  const args = ['archiver', 'sync', archiverName, '--wait'];

  console.log(`Starting ssmd archiver sync for: ${archiverName}`);

  return new Promise((resolve) => {
    let stdout = '';
    let stderr = '';
    const child = spawn(command, args, {
      env: {
        ...process.env,
        TRIGGERED_BY: 'temporal',
      },
    });

    child.stdout.on('data', (data) => {
      stdout += data.toString();
    });

    child.stderr.on('data', (data) => {
      stderr += data.toString();
    });

    child.on('close', (code) => {
      const exitCode = code ?? 1;
      resolve({
        success: exitCode === 0,
        stdout,
        stderr,
        exitCode,
        durationMs: Date.now() - startTime,
      });
    });

    child.on('error', (err) => {
      resolve({
        success: false,
        stdout,
        stderr: stderr + '\n' + err.message,
        exitCode: 1,
        durationMs: Date.now() - startTime,
      });
    });

    const timeout = setTimeout(() => {
      child.kill('SIGTERM');
      resolve({
        success: false,
        stdout,
        stderr: stderr + '\nProcess killed: timeout exceeded',
        exitCode: 124,
        durationMs: Date.now() - startTime,
      });
    }, 10 * 60 * 1000);

    child.on('close', () => clearTimeout(timeout));
  });
}

export interface DataQualityResult {
  success: boolean;
  report: {
    date: string;
    feeds: Record<string, { score: number; [key: string]: unknown }>;
    composite: number;
    grade: 'GREEN' | 'YELLOW' | 'RED';
    issues: string[];
    prometheusDegraded: boolean;
  } | null;
  error?: string;
  durationMs: number;
}

/**
 * Execute ssmd dq daily --json command
 * Returns parsed JSON report from CLI output
 */
export async function runDataQualityCheck(): Promise<DataQualityResult> {
  const startTime = Date.now();
  const command = process.env.SSMD_COMMAND || 'ssmd';
  const args = ['dq', 'daily', '--json'];

  const databaseUrl = process.env.DATABASE_URL;
  if (!databaseUrl) {
    return {
      success: false,
      report: null,
      error: 'DATABASE_URL environment variable not set',
      durationMs: Date.now() - startTime,
    };
  }

  console.log('Starting ssmd dq daily check');

  return new Promise((resolve) => {
    let stdout = '';
    let stderr = '';
    const child = spawn(command, args, {
      env: {
        ...process.env,
        TRIGGERED_BY: 'temporal',
      },
    });

    child.stdout.on('data', (data) => {
      stdout += data.toString();
    });

    child.stderr.on('data', (data) => {
      stderr += data.toString();
    });

    child.on('close', (code) => {
      if (code !== 0) {
        resolve({
          success: false,
          report: null,
          error: stderr || 'Process exited with code ' + code,
          durationMs: Date.now() - startTime,
        });
        return;
      }

      // Parse JSON from stdout (last non-empty line should be JSON)
      const lines = stdout.trim().split('\n');
      let report = null;
      for (let i = lines.length - 1; i >= 0; i--) {
        const line = lines[i].trim();
        if (line.startsWith('{')) {
          try {
            report = JSON.parse(line);
            break;
          } catch {
            // continue searching
          }
        }
      }

      if (!report) {
        resolve({
          success: false,
          report: null,
          error: 'Failed to parse JSON from CLI output: ' + stdout.slice(0, 500),
          durationMs: Date.now() - startTime,
        });
        return;
      }

      if (stderr) {
        console.log('DQ check stderr: ' + stderr.slice(0, 200));
      }

      resolve({
        success: true,
        report,
        durationMs: Date.now() - startTime,
      });
    });

    child.on('error', (err) => {
      resolve({
        success: false,
        report: null,
        error: err.message,
        durationMs: Date.now() - startTime,
      });
    });

    const timeout = setTimeout(() => {
      child.kill('SIGTERM');
      resolve({
        success: false,
        report: null,
        error: 'Process killed: 5 minute timeout exceeded',
        durationMs: Date.now() - startTime,
      });
    }, 5 * 60 * 1000);

    child.on('close', () => clearTimeout(timeout));
  });
}

export interface NotificationInput {
  title: string;
  message: string;
  priority?: 'min' | 'low' | 'default' | 'high' | 'urgent';
  tags?: string[];
  /** Optional topic override (ntfy only). */
  topic?: string;
}

/**
 * Send notification via configured backend.
 *
 * Feature flag: NOTIFY_BACKEND env var
 *   - "email" — Gmail SMTP (requires SMTP_USER, SMTP_PASS, SMTP_TO)
 *   - "ntfy"  — ntfy HTTP POST (requires NTFY_URL)
 *   - unset   — skips silently
 */
export async function sendNotification(input: NotificationInput): Promise<void> {
  const backend = process.env.NOTIFY_BACKEND;

  if (backend === 'email') {
    return sendEmail(input);
  } else if (backend === 'ntfy') {
    return sendNtfy(input);
  } else {
    console.log('NOTIFY_BACKEND not set, skipping notification');
  }
}

async function sendEmail(input: NotificationInput): Promise<void> {
  const user = process.env.SMTP_USER;
  const pass = process.env.SMTP_PASS;
  const to = process.env.SMTP_TO;

  if (!user || !pass || !to) {
    console.log('SMTP_USER/SMTP_PASS/SMTP_TO not set, skipping email');
    return;
  }

  const transport = createTransport({
    host: 'smtp.gmail.com',
    port: 587,
    secure: false,
    auth: { user, pass },
  });

  const priorityTag = input.priority === 'urgent' || input.priority === 'high' ? 'HIGH' : '';
  const subject = priorityTag
    ? `[${priorityTag}] ${input.title}`
    : input.title;

  await transport.sendMail({
    from: user,
    to,
    subject,
    text: input.message,
  });

  console.log('Email sent: ' + input.title);
}

async function sendNtfy(input: NotificationInput): Promise<void> {
  const ntfyUrl = process.env.NTFY_URL;
  if (!ntfyUrl) {
    console.log('NTFY_URL not set, skipping ntfy notification');
    return;
  }

  const https = await import('https');
  const http = await import('http');
  const url = new URL(ntfyUrl);

  if (input.topic) {
    url.pathname = '/' + input.topic;
  } else if (url.pathname === '/') {
    url.pathname = '/ssmd-secmaster';
  }

  const isHttps = url.protocol === 'https:';
  const transport = isHttps ? https : http;

  const headers: Record<string, string> = {
    'Title': input.title,
    'Priority': input.priority || 'high',
  };

  if (input.tags && input.tags.length > 0) {
    headers['Tags'] = input.tags.join(',');
  }

  return new Promise((resolve, reject) => {
    const req = transport.request(
      {
        hostname: url.hostname,
        port: url.port || (isHttps ? 443 : 80),
        path: url.pathname,
        method: 'POST',
        headers,
      },
      (res) => {
        res.resume();
        if (res.statusCode && res.statusCode >= 200 && res.statusCode < 300) {
          console.log('Ntfy sent: ' + input.title);
          resolve();
        } else {
          reject(new Error('ntfy request failed with status ' + res.statusCode));
        }
      }
    );

    req.on('error', reject);
    req.write(input.message);
    req.end();
  });
}
