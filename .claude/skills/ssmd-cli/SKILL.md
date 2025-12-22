---
name: ssmd-cli
description: Use the ssmd CLI to manage market data feed configurations. Use when creating feeds, schemas, environments, or working with the ssmd command line tool.
allowed-tools: Read, Glob, Grep, Bash
---

# SSMD CLI

ssmd is a GitOps metadata CLI for managing market data feed configurations.

## Quick Reference

```bash
# Initialize directory structure
ssmd init

# Validate all configs
ssmd validate

# Show changes and commit
ssmd diff
ssmd commit -m "message"
```

## Feed Commands

```bash
# List feeds
ssmd feed list

# Show feed details
ssmd feed show <name>

# Create feed
ssmd feed create <name> \
  --type websocket \
  --endpoint "wss://..." \
  --display-name "Display Name" \
  --auth-method api_key \
  --rate-limit 10

# Add version to existing feed
ssmd feed add-version <name> \
  --effective-from 2025-01-01 \
  --endpoint "wss://new-endpoint"

# Add capture location
ssmd feed add-location <name> \
  --datacenter nyc1 \
  --provider onprem
```

## Schema Commands

```bash
# List schemas
ssmd schema list

# Show schema details
ssmd schema show <name>

# Register new schema (creates .yaml metadata)
ssmd schema register <name> --file schemas/<name>.capnp

# Set schema status
ssmd schema set-status <name>:v1 active   # active, draft, deprecated

# Recompute hash
ssmd schema hash <name>

# Add new version
ssmd schema add-version <name> \
  --file schemas/<name>-v2.capnp \
  --effective-from 2025-06-01
```

## Environment Commands

```bash
# List environments
ssmd env list

# Show environment details
ssmd env show <name>

# Create environment
ssmd env create <name> \
  --feed <feed-name> \
  --schema <schema>:v1

# Add API key reference
ssmd env add-key <env-name> <key-name> \
  --type api_key \
  --fields api_key,api_secret \
  --source env
```

## Typical Workflow

1. Initialize: `ssmd init`
2. Create feed: `ssmd feed create ...`
3. Create schema file: write `schemas/<name>.capnp`
4. Register schema: `ssmd schema register <name> --file schemas/<name>.capnp`
5. Activate schema: `ssmd schema set-status <name>:v1 active`
6. Create environment: `ssmd env create ... --feed <feed> --schema <schema>:v1`
7. Validate: `ssmd validate`
8. Commit: `ssmd commit -m "Add new feed"`

## File Locations

- Feeds: `exchanges/feeds/<name>.yaml`
- Schemas: `exchanges/schemas/<name>.yaml` + `exchanges/schemas/<name>.capnp`
- Environments: `exchanges/environments/<name>.yaml`
- Local config: `.ssmd/` (gitignored)

## Important Notes

- Schema must be `active` status before environment can reference it
- Always run `ssmd validate` before committing
- `ssmd commit` runs validation automatically
