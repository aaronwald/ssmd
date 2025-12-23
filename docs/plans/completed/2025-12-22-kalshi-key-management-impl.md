# Implementation Plan: Kalshi Key Management

## Overview

Implement key management commands for ssmd CLI to validate and track API credentials, transport secrets, and storage credentials. Keys are defined in environment YAML files. **ssmd never stores actual secrets** - it only tracks metadata and validates that secrets exist in external sources (environment variables, secret managers).

## Design Principles

1. **No secrets on disk** - ssmd never writes secret values to the filesystem
2. **Reference, don't store** - Keys specify a `source` that points to where secrets live
3. **Validate at CLI** - `key verify` checks that referenced secrets exist
4. **Inject at runtime** - Connectors/services read secrets from their sources at startup

## Supported Sources

| Source | Format | Example |
|--------|--------|---------|
| Environment variables | `env:VAR1,VAR2` | `env:KALSHI_API_KEY,KALSHI_API_SECRET` |
| Sealed Secrets (K8s) | `sealed-secret:<namespace>/<name>` | `sealed-secret:ssmd/kalshi-prod` |
| Vault (future) | `vault:<path>` | `vault:secret/data/kalshi` |

## Tasks

### 1. Add key types to internal/types (DONE)

Create `internal/types/key.go` with:
- `KeySpec` struct (type, description, required, fields, rotation_days)
- `KeyStatus` struct (name, type, status, last_verified, source)
- `KeyType` enum (api_key, transport, storage, tls, webhook)

Files modified:
- Created `internal/types/key.go`

### 2. Extend environment types to include keys section (DONE)

Add `Keys map[string]KeySpec` to the `Environment` struct in `internal/types/environment.go`.

Files modified:
- `internal/types/environment.go`

### 3. Implement ssmd key list command (DONE)

Create `internal/cmd/key.go` with:
- `keyCmd` root command
- `keyListCmd` - lists keys defined in an environment with their status

Files modified:
- Created `internal/cmd/key.go`
- `cmd/ssmd/main.go` (add key command)

### 4. Implement ssmd key show command (DONE)

Add `keyShowCmd` to show detailed metadata for a specific key:
- Name, type, required, fields
- Source reference
- Last verified timestamp

Files modified:
- `internal/cmd/key.go`

### 5. Implement ssmd key verify command

Add `keyVerifyCmd` to verify secrets exist in their referenced sources:
- Read key definitions from environment YAML
- For `env:` sources, check environment variables are set and non-empty
- For `sealed-secret:` sources, check kubectl can read the secret (optional)
- Report missing/invalid keys
- Exit non-zero if validation fails

Example:
```bash
$ ssmd key verify kalshi-dev
Verifying keys for environment 'kalshi-dev'...
  kalshi (env:KALSHI_API_KEY,KALSHI_API_SECRET)
    ✓ KALSHI_API_KEY is set
    ✓ KALSHI_API_SECRET is set
  nats (env:NATS_URL,NATS_USER,NATS_PASSWORD)
    ✓ NATS_URL is set
    ✗ NATS_USER is not set
    ✗ NATS_PASSWORD is not set

Error: 2 required key fields are not set.
```

Files to modify:
- `internal/cmd/key.go`

### 6. Implement ssmd key check command (renamed from set)

Instead of `key set`, implement `key check` to validate a single key:
- Parse source from key spec
- Validate all fields exist in the source
- Update last_verified timestamp in metadata

Example:
```bash
$ ssmd key check kalshi-dev kalshi
Checking key 'kalshi' (source: env:KALSHI_API_KEY,KALSHI_API_SECRET)...
  ✓ KALSHI_API_KEY is set
  ✓ KALSHI_API_SECRET is set
Key 'kalshi' verified successfully.
```

Files to modify:
- `internal/cmd/key.go`

### 7. Add key validation to ssmd validate

Extend the validate command to check:
- All environments have valid key definitions
- Key sources are valid format
- Optionally verify secrets exist (with --check-keys flag)

Files to modify:
- `internal/cmd/validate.go`

### 8. Remove .ssmd/secrets from plan

No longer needed - we don't store secrets.

The `.ssmd/keys/` directory can optionally store verification metadata (last_verified timestamps), but this is optional and not security-sensitive.

## File Structure After Implementation

```
internal/
  types/
    key.go           # Key types (DONE)
    environment.go   # Keys field (DONE)
  cmd/
    key.go           # Key commands (IN PROGRESS)
    validate.go      # Key validation

.ssmd/
  keys/              # Optional: verification metadata only
    kalshi-dev/
      kalshi.yaml    # last_verified timestamp, no secrets
```

## Testing

- Unit tests for key type parsing
- Unit tests for source validation (env var checking)
- Integration test for key verify with mock env vars
- Test validation catches missing required keys

## Dependencies

No new external dependencies required. Uses existing:
- `gopkg.in/yaml.v3` for YAML parsing
- `github.com/spf13/cobra` for CLI
- `os.Getenv()` for environment variable validation

## Notes

- ssmd CLI never handles actual secret values
- Environment variables are the primary source for local dev
- Sealed Secrets for production K8s deployments
- Vault integration can be added later as another source type
