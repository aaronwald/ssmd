# Implementation Plan: Kalshi Key Management

## Overview

Implement key management commands for ssmd CLI to handle API credentials, transport secrets, and storage credentials. Keys are defined in environment YAML files and stored as Kubernetes Sealed Secrets.

## Tasks

### 1. Add key types to internal/types

Create `internal/types/key.go` with:
- `KeySpec` struct (type, description, required, fields, rotation_days)
- `KeyStatus` struct (name, type, status, last_rotated, expires_at, sealed_secret_ref)
- `KeyType` enum (api_key, transport, storage, tls, webhook)

Files to modify:
- Create `internal/types/key.go`

### 2. Extend environment types to include keys section

Add `Keys map[string]KeySpec` to the `Environment` struct in `internal/types/environment.go`.

Update YAML parsing to handle the keys section.

Files to modify:
- `internal/types/environment.go`

### 3. Implement ssmd key list command

Create `internal/cmd/key.go` with:
- `keyCmd` root command
- `keyListCmd` - lists keys defined in an environment with their status
- Reads from environment YAML + checks local key status file

Files to modify:
- Create `internal/cmd/key.go`
- `internal/cmd/root.go` (add key command)

### 4. Implement ssmd key show command

Add `keyShowCmd` to show detailed metadata for a specific key:
- Name, type, required, fields
- Status (set/not_set)
- Last rotated timestamp
- Expiration date (if rotation_days set)

Files to modify:
- `internal/cmd/key.go`

### 5. Implement ssmd key set command

Add `keySetCmd` to set key values:
- Parse field values from flags (--api-key, --api-secret, etc.)
- Support --from-file for certificate files
- Support --from-env for environment variables
- Store key status in `.ssmd/keys/<env>/<key>.yaml` (no actual secrets, just metadata)
- For local dev: store actual values in `.ssmd/secrets/<env>/<key>.yaml` (gitignored)

Files to modify:
- `internal/cmd/key.go`
- Create `internal/cmd/key_storage.go` for local key storage logic

### 6. Implement ssmd key verify command

Add `keyVerifyCmd` to verify all required keys are set:
- Read key definitions from environment YAML
- Check status of each key
- Report missing required keys
- Exit non-zero if validation fails

Files to modify:
- `internal/cmd/key.go`

### 7. Implement ssmd key delete command

Add `keyDeleteCmd` to remove a key:
- Delete local key status/value files
- Warn if key is required

Files to modify:
- `internal/cmd/key.go`

### 8. Add key validation to ssmd validate

Extend the validate command to check:
- All environments have valid key definitions
- Required keys are set (optional, with --check-keys flag)

Files to modify:
- `internal/cmd/validate.go`

### 9. Add key validation to ssmd env apply (future)

When `ssmd env apply` is implemented, it should:
- Call key verify before applying
- Fail if required keys missing

Files to modify:
- `internal/cmd/env.go` (when apply is added)

### 10. Update .gitignore for secrets

Ensure `.ssmd/secrets/` is gitignored to prevent accidental secret commits.

Files to modify:
- `.gitignore`

## File Structure After Implementation

```
internal/
  types/
    key.go           # NEW: Key types
    environment.go   # MODIFIED: Add keys field
  cmd/
    key.go           # NEW: Key commands
    key_storage.go   # NEW: Local key storage
    root.go          # MODIFIED: Add key command
    validate.go      # MODIFIED: Key validation

.ssmd/
  keys/              # Key metadata (committed)
    kalshi-dev/
      kalshi.yaml    # status, last_rotated, etc.
  secrets/           # Actual values (gitignored)
    kalshi-dev/
      kalshi.yaml    # api_key, api_secret values
```

## Testing

- Unit tests for key type parsing
- Unit tests for key storage read/write
- Integration test for key set/list/show/verify/delete cycle
- Test validation catches missing required keys

## Dependencies

No new external dependencies required. Uses existing:
- `gopkg.in/yaml.v3` for YAML parsing
- `github.com/spf13/cobra` for CLI

## Notes

- For Phase 1, keys are stored locally (not Sealed Secrets)
- Sealed Secrets integration deferred to Phase 4 (deployment)
- Key rotation tracking is metadata-only in Phase 1
