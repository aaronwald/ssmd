import { loadMomentumConfig, MomentumConfigSchema } from "../../momentum/config.ts";
import { runMomentum } from "../../momentum/runner.ts";
import { runMomentumBacktest } from "../../momentum/backtest.ts";

export async function handleMomentum(
  subcommand: string,
  flags: Record<string, unknown>,
): Promise<void> {
  switch (subcommand) {
    case "run":
      await handleRun(flags);
      break;
    case "backtest": {
      // Check for sub-subcommand: submit, results
      const subSub = (flags as { _: string[] })._[2] as string | undefined;
      if (subSub === "submit") {
        await handleBacktestSubmit(flags);
      } else if (subSub === "results") {
        await handleBacktestResults(flags);
      } else {
        await handleBacktest(flags);
      }
      break;
    }
    default:
      console.log("ssmd momentum - Paper trading momentum models");
      console.log("");
      console.log("USAGE:");
      console.log("  ssmd momentum run [options]");
      console.log("  ssmd momentum backtest [options]");
      console.log("  ssmd momentum backtest submit [options]");
      console.log("  ssmd momentum backtest results [run-id]");
      console.log("");
      console.log("RUN OPTIONS:");
      console.log("  --config <path>     Config file (YAML)");
      console.log("  --stream <name>     NATS stream (default: PROD_KALSHI_SPORTS)");
      console.log("  --balance <amount>  Starting balance in dollars (default: 500)");
      console.log("  --nats-url <url>    NATS URL (default: nats://localhost:4222)");
      console.log("");
      console.log("BACKTEST OPTIONS:");
      console.log("  --config <path>       Config file (YAML) — same as live runner");
      console.log("  --from <YYYY-MM-DD>   Start date");
      console.log("  --to <YYYY-MM-DD>     End date");
      console.log("  --dates <d1,d2,...>   Specific dates (alternative to --from/--to)");
      console.log("  --bucket <name>       GCS bucket (default: ssmd-archive)");
      console.log("  --prefix <path>       GCS prefix (default: kalshi/sports)");
      console.log("  --trades-out <path>   Write per-trade JSONL to file");
      console.log("  --cache-dir <path>    Cache directory for GCS files");
      console.log("  --results-dir <path>  Results output directory (default: ./results)");
      console.log("  --run-id <id>         Unique run ID (default: auto-generated UUID)");
      console.log("");
      console.log("BACKTEST SUBMIT OPTIONS:");
      console.log("  --config <path>       Config file to use for backtest");
      console.log("  --from <YYYY-MM-DD>   Start date");
      console.log("  --to <YYYY-MM-DD>     End date");
      console.log("  --image <tag>         Backtest image tag (default: 0.1.0)");
      Deno.exit(1);
  }
}

async function handleRun(flags: Record<string, unknown>): Promise<void> {
  const configPath = flags.config as string | undefined;

  let config;
  if (configPath) {
    config = await loadMomentumConfig(configPath);
  } else {
    const overrides: Record<string, unknown> = {
      nats: {
        url: (flags["nats-url"] as string) ?? "nats://localhost:4222",
        stream: (flags.stream as string) ?? "PROD_KALSHI_SPORTS",
        filter: flags.filter as string | undefined,
      },
    };

    if (flags.balance) {
      overrides.portfolio = { startingBalance: Number(flags.balance) };
    }

    config = MomentumConfigSchema.parse(overrides);
  }

  await runMomentum(config);
}

function generateDateRange(from: string, to: string): string[] {
  const dates: string[] = [];
  const start = new Date(from + "T00:00:00Z");
  const end = new Date(to + "T00:00:00Z");

  for (let d = start; d <= end; d.setUTCDate(d.getUTCDate() + 1)) {
    dates.push(d.toISOString().slice(0, 10));
  }

  return dates;
}

async function handleBacktest(flags: Record<string, unknown>): Promise<void> {
  const configPath = flags.config as string | undefined;
  if (!configPath) {
    console.error("Error: --config is required for backtest");
    Deno.exit(1);
  }

  const config = await loadMomentumConfig(configPath);

  // Resolve dates
  let dates: string[];
  const datesFlag = flags.dates as string | undefined;
  const fromFlag = flags.from as string | undefined;
  const toFlag = flags.to as string | undefined;

  if (datesFlag) {
    dates = datesFlag.split(",").map((d) => d.trim());
  } else if (fromFlag && toFlag) {
    dates = generateDateRange(fromFlag, toFlag);
  } else if (fromFlag) {
    dates = [fromFlag];
  } else {
    console.error("Error: --from/--to or --dates is required for backtest");
    Deno.exit(1);
  }

  const bucket = (flags.bucket as string) ?? "ssmd-archive";
  const prefix = (flags.prefix as string) ?? "kalshi/sports";
  const tradesOut = flags["trades-out"] as string | undefined;
  const cacheDir = flags["cache-dir"] as string | undefined;
  const resultsDir = flags["results-dir"] as string | undefined;
  const runId = flags["run-id"] as string | undefined;

  await runMomentumBacktest({
    config,
    configPath,
    dates,
    bucket,
    prefix,
    tradesOut,
    cacheDir,
    resultsDir,
    runId,
  });
}

async function handleBacktestSubmit(flags: Record<string, unknown>): Promise<void> {
  const configPath = flags.config as string | undefined;
  if (!configPath) {
    console.error("Error: --config is required for backtest submit");
    Deno.exit(1);
  }

  const fromDate = flags.from as string | undefined;
  const toDate = flags.to as string | undefined;
  if (!fromDate || !toDate) {
    console.error("Error: --from and --to are required for backtest submit");
    Deno.exit(1);
  }

  const imageTag = (flags.image as string) ?? "0.1.0";
  const runId = crypto.randomUUID();

  // Read config file to create ConfigMap
  const configContent = await Deno.readTextFile(configPath);
  const configName = configPath.split("/").pop()?.replace(/\.(yaml|yml)$/, "") ?? "backtest";

  // Create/update ConfigMap
  const cmCmd = new Deno.Command("kubectl", {
    args: [
      "create", "configmap", "ssmd-backtest-config",
      "-n", "ssmd",
      `--from-literal=momentum.yaml=${configContent}`,
      "--dry-run=client", "-o", "yaml",
    ],
    stdout: "piped",
    stderr: "piped",
  });
  const cmOutput = await cmCmd.output();
  if (!cmOutput.success) {
    console.error(`Failed to generate ConfigMap: ${new TextDecoder().decode(cmOutput.stderr)}`);
    Deno.exit(1);
  }

  const cmYaml = new TextDecoder().decode(cmOutput.stdout);
  const applyCmd = new Deno.Command("kubectl", {
    args: ["apply", "-f", "-"],
    stdin: "piped",
    stdout: "piped",
    stderr: "piped",
  });
  const applyChild = applyCmd.spawn();
  const writer = applyChild.stdin.getWriter();
  await writer.write(new TextEncoder().encode(cmYaml));
  await writer.close();
  const applyOutput = await applyChild.output();
  if (!applyOutput.success) {
    console.error(`Failed to apply ConfigMap: ${new TextDecoder().decode(applyOutput.stderr)}`);
    Deno.exit(1);
  }

  // Generate Job YAML
  const jobYaml = `apiVersion: batch/v1
kind: Job
metadata:
  name: backtest-${runId.slice(0, 8)}
  namespace: ssmd
  labels:
    app: ssmd-backtest
    backtest-run-id: "${runId}"
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 86400
  template:
    metadata:
      labels:
        app: ssmd-backtest
        backtest-run-id: "${runId}"
    spec:
      restartPolicy: Never
      securityContext:
        runAsUser: 1000
        runAsGroup: 1000
        fsGroup: 1000
      imagePullSecrets:
        - name: ghcr-secret
      containers:
        - name: backtest
          image: ghcr.io/aaronwald/ssmd-backtest:${imageTag}
          args:
            - "--config"
            - "/config/momentum.yaml"
            - "--from"
            - "${fromDate}"
            - "--to"
            - "${toDate}"
            - "--cache-dir"
            - "/cache"
            - "--results-dir"
            - "/results"
            - "--run-id"
            - "${runId}"
          env:
            - name: GOOGLE_APPLICATION_CREDENTIALS
              value: /secrets/gcs/key.json
          resources:
            requests:
              cpu: 100m
              memory: 512Mi
            limits:
              cpu: "1"
              memory: 1Gi
          volumeMounts:
            - name: config
              mountPath: /config
              readOnly: true
            - name: cache
              mountPath: /cache
            - name: results
              mountPath: /results
            - name: gcs-credentials
              mountPath: /secrets/gcs
              readOnly: true
      volumes:
        - name: config
          configMap:
            name: ssmd-backtest-config
        - name: cache
          persistentVolumeClaim:
            claimName: ssmd-backtest-cache
        - name: results
          persistentVolumeClaim:
            claimName: ssmd-backtest-results
        - name: gcs-credentials
          secret:
            secretName: gcs-credentials
`;

  // Apply Job
  const jobCmd = new Deno.Command("kubectl", {
    args: ["apply", "-f", "-"],
    stdin: "piped",
    stdout: "piped",
    stderr: "piped",
  });
  const jobChild = jobCmd.spawn();
  const jobWriter = jobChild.stdin.getWriter();
  await jobWriter.write(new TextEncoder().encode(jobYaml));
  await jobWriter.close();
  const jobOutput = await jobChild.output();
  if (!jobOutput.success) {
    console.error(`Failed to create Job: ${new TextDecoder().decode(jobOutput.stderr)}`);
    Deno.exit(1);
  }

  console.log(`[backtest] Submitted backtest Job`);
  console.log(`[backtest] Run ID:  ${runId}`);
  console.log(`[backtest] Job:     backtest-${runId.slice(0, 8)}`);
  console.log(`[backtest] Config:  ${configName}`);
  console.log(`[backtest] Dates:   ${fromDate} → ${toDate}`);
  console.log(`[backtest] Image:   ghcr.io/aaronwald/ssmd-backtest:${imageTag}`);
  console.log(``);
  console.log(`Watch logs:`);
  console.log(`  kubectl logs -n ssmd job/backtest-${runId.slice(0, 8)} -f`);
}

async function handleBacktestResults(flags: Record<string, unknown>): Promise<void> {
  const runIdArg = (flags as { _: string[] })._[3] as string | undefined;

  if (runIdArg) {
    // Show specific run summary
    const cmd = new Deno.Command("kubectl", {
      args: [
        "exec", "-n", "ssmd",
        "deploy/ssmd-debug", "--",
        "cat", `/results/${runIdArg}/summary.json`,
      ],
      stdout: "piped",
      stderr: "piped",
    });
    const output = await cmd.output();
    if (!output.success) {
      console.error(`Failed to read results for ${runIdArg}: ${new TextDecoder().decode(output.stderr)}`);
      Deno.exit(1);
    }
    console.log(new TextDecoder().decode(output.stdout));
  } else {
    // List all runs
    const cmd = new Deno.Command("kubectl", {
      args: [
        "get", "jobs", "-n", "ssmd",
        "-l", "app=ssmd-backtest",
        "-o", "wide",
      ],
      stdout: "piped",
      stderr: "piped",
    });
    const output = await cmd.output();
    if (!output.success) {
      console.error(`Failed to list backtest jobs: ${new TextDecoder().decode(output.stderr)}`);
      Deno.exit(1);
    }
    console.log(new TextDecoder().decode(output.stdout));
  }
}
