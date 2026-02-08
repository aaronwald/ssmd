# DevOps Engineer Memory

## 2026-02-07: Temporal Worker Deployment Review (Kraken/Polymarket)

### Key Findings
- **CLI image is always the first step** — worker Dockerfile uses `COPY --from=ghcr.io/aaronwald/ssmd-cli-ts:<version>` to bundle CLI binary. New CLI commands require a new CLI tag before worker can be rebuilt.
- **Deployment chain**: ssmd CLI tag → ssmd CLI image build → varlab worker Dockerfile update → varlab worker tag → varlab worker image build → varlab deployment.yaml update → Flux reconcile.
- **Schedule-seeder Job is immutable** — checksum annotation bump forces Flux to recreate, but old Job must be deleted first if TTL hasn't cleaned it up. `ttlSecondsAfterFinished: 3600`.
- **Seeder is idempotent for existing schedules** — uses `temporal schedule describe` to skip existing ones, only creates new.
- **`minCloseDaysAgo` is NOT wired through worker** — schedule ConfigMap passes it in JSON input, but `SecmasterInput` and `SecmasterSyncOptions` interfaces don't include it. Silent data loss.
- **Network policy label mismatch** — postgres NP uses `app.kubernetes.io/name: ssmd-worker` but worker pods use `app: ssmd-worker`. Works because DATABASE_URL uses direct connection, not k8s service.

### Codebase Locations
- Worker source: `varlab/workers/kalshi-temporal/src/` (activities.ts, workflows.ts, worker.ts)
- Worker Dockerfile: `varlab/workers/kalshi-temporal/Dockerfile`
- Worker deployment: `varlab/clusters/homelab/apps/ssmd/worker/deployment.yaml`
- Schedule seeder: `varlab/clusters/homelab/infrastructure/temporal/schedule-seeder.yaml`
- Worker build workflow: `varlab/.github/workflows/ssmd-worker-build.yaml` (tag: `ssmd-worker-v*`)
- CLI build workflow: `ssmd/.github/workflows/build-cli-ts.yaml` (tag: `cli-ts-v*`)
- CLI commands: `ssmd-agent/src/cli/commands/` (kraken-sync.ts, polymarket-sync.ts)

### Tag Formats
- CLI: `cli-ts-v0.2.XX` (ssmd repo)
- Worker: `ssmd-worker-v0.8.XX` (varlab repo)
- Worker image refs CLI: `ghcr.io/aaronwald/ssmd-cli-ts:<version>` in Dockerfile COPY --from

### Patterns
- Worker uses `execAsync` for simple commands (fees, kraken, polymarket, scale)
- Worker uses `spawn` + readline for long-running commands with heartbeats (secmaster sync)
- Temporal activity proxies are grouped by timeout profile (30min+heartbeat for secmaster, 10min for multi-exchange, 30s for notifications)
- Schedule intervals use offset notation: `6h/Xm` means every 6 hours, offset by X minutes
