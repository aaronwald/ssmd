# Phase 1: Metadata Foundation - Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build the metadata foundation that all ssmd components depend on - the system must know what it's managing before managing it.

**Architecture:** PostgreSQL stores all metadata (feeds, schemas, environments, inventory). Go CLI (`ssmd`) provides the operator interface with validation against metadata before any action. All tables use temporal patterns (effective_from/effective_to) for time-travel queries.

**Tech Stack:** Go 1.21+, PostgreSQL 16, sqlc for type-safe queries, cobra for CLI, golang-migrate for schema migrations

---

## Prerequisites

Before starting, ensure:
- PostgreSQL 16 is running (use existing infrastructure or `docker run -d --name ssmd-postgres -e POSTGRES_PASSWORD=ssmd -e POSTGRES_DB=ssmd -p 5432:5432 postgres:16`)
- Go 1.21+ installed
- `sqlc` installed: `go install github.com/sqlc-dev/sqlc/cmd/sqlc@latest`
- `golang-migrate` installed: `go install -tags 'postgres' github.com/golang-migrate/migrate/v4/cmd/migrate@latest`

---

## Task 1: Initialize Go Module

**Files:**
- Create: `cmd/ssmd/main.go`
- Create: `go.mod`
- Create: `go.sum`

**Step 1: Create Go module**

```bash
cd /workspaces/ssmd
go mod init github.com/your-org/ssmd
```

**Step 2: Create minimal CLI entry point**

Create `cmd/ssmd/main.go`:

```go
package main

import (
	"fmt"
	"os"

	"github.com/spf13/cobra"
)

var rootCmd = &cobra.Command{
	Use:   "ssmd",
	Short: "Stupid Simple Market Data CLI",
	Long:  `ssmd manages market data feeds, schemas, and environments.`,
}

func main() {
	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
```

**Step 3: Add cobra dependency**

```bash
go get github.com/spf13/cobra@v1.8.0
```

**Step 4: Build and verify**

```bash
go build -o bin/ssmd ./cmd/ssmd
./bin/ssmd --help
```

Expected: Help output showing "Stupid Simple Market Data CLI"

**Step 5: Commit**

```bash
git add go.mod go.sum cmd/
git commit -m "feat: initialize Go module with cobra CLI skeleton"
```

---

## Task 2: Database Connection Module

**Files:**
- Create: `internal/db/db.go`
- Create: `internal/config/config.go`

**Step 1: Create config module**

Create `internal/config/config.go`:

```go
package config

import (
	"fmt"
	"os"
)

type Config struct {
	DatabaseURL string
}

func Load() (*Config, error) {
	dbURL := os.Getenv("SSMD_DATABASE_URL")
	if dbURL == "" {
		dbURL = "postgres://postgres:ssmd@localhost:5432/ssmd?sslmode=disable"
	}
	return &Config{
		DatabaseURL: dbURL,
	}, nil
}

func (c *Config) Validate() error {
	if c.DatabaseURL == "" {
		return fmt.Errorf("database URL is required")
	}
	return nil
}
```

**Step 2: Create database module**

Create `internal/db/db.go`:

```go
package db

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5/pgxpool"
)

type DB struct {
	Pool *pgxpool.Pool
}

func Connect(ctx context.Context, databaseURL string) (*DB, error) {
	pool, err := pgxpool.New(ctx, databaseURL)
	if err != nil {
		return nil, fmt.Errorf("unable to create connection pool: %w", err)
	}

	if err := pool.Ping(ctx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("unable to ping database: %w", err)
	}

	return &DB{Pool: pool}, nil
}

func (d *DB) Close() {
	d.Pool.Close()
}
```

**Step 3: Add pgx dependency**

```bash
go get github.com/jackc/pgx/v5@v5.5.0
```

**Step 4: Verify build**

```bash
go build ./...
```

Expected: No errors

**Step 5: Commit**

```bash
git add internal/
git commit -m "feat: add database connection and config modules"
```

---

## Task 3: Migration Infrastructure

**Files:**
- Create: `migrations/` directory
- Create: `internal/db/migrate.go`

**Step 1: Create migrations directory**

```bash
mkdir -p migrations
```

**Step 2: Create migration helper**

Create `internal/db/migrate.go`:

```go
package db

import (
	"embed"
	"fmt"

	"github.com/golang-migrate/migrate/v4"
	_ "github.com/golang-migrate/migrate/v4/database/postgres"
	"github.com/golang-migrate/migrate/v4/source/iofs"
)

//go:embed migrations/*.sql
var migrationsFS embed.FS

func RunMigrations(databaseURL string) error {
	source, err := iofs.New(migrationsFS, "migrations")
	if err != nil {
		return fmt.Errorf("failed to create migration source: %w", err)
	}

	m, err := migrate.NewWithSourceInstance("iofs", source, databaseURL)
	if err != nil {
		return fmt.Errorf("failed to create migrate instance: %w", err)
	}
	defer m.Close()

	if err := m.Up(); err != nil && err != migrate.ErrNoChange {
		return fmt.Errorf("failed to run migrations: %w", err)
	}

	return nil
}
```

**Step 3: Add migrate dependency**

```bash
go get github.com/golang-migrate/migrate/v4@v4.17.0
```

**Step 4: Create placeholder migration**

Create `internal/db/migrations/000001_init.up.sql`:

```sql
-- Placeholder for initial schema
SELECT 1;
```

Create `internal/db/migrations/000001_init.down.sql`:

```sql
-- Placeholder for rollback
SELECT 1;
```

**Step 5: Verify build**

```bash
go build ./...
```

**Step 6: Commit**

```bash
git add internal/db/migrate.go internal/db/migrations/
git commit -m "feat: add migration infrastructure with embedded SQL"
```

---

## Task 4: Feed Registry Schema

**Files:**
- Create: `internal/db/migrations/000002_feeds.up.sql`
- Create: `internal/db/migrations/000002_feeds.down.sql`

**Step 1: Create feeds migration (up)**

Create `internal/db/migrations/000002_feeds.up.sql`:

```sql
-- Feed Registry: defines what data sources exist and how to connect

CREATE TABLE feeds (
    id SERIAL PRIMARY KEY,
    name VARCHAR(64) UNIQUE NOT NULL,
    display_name VARCHAR(128),
    feed_type VARCHAR(32) NOT NULL,  -- 'websocket', 'rest', 'multicast'
    status VARCHAR(16) DEFAULT 'active',  -- 'active', 'deprecated', 'disabled'
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE feed_versions (
    id SERIAL PRIMARY KEY,
    feed_id INTEGER REFERENCES feeds(id) ON DELETE CASCADE,
    version VARCHAR(32) NOT NULL,
    effective_from DATE NOT NULL,
    effective_to DATE,  -- NULL = current

    -- Connection details
    protocol VARCHAR(32) NOT NULL,  -- 'wss', 'https', 'multicast'
    endpoint_template TEXT NOT NULL,
    auth_method VARCHAR(32),  -- 'api_key', 'oauth', 'mtls'
    secret_ref VARCHAR(128),

    -- Capabilities
    supports_orderbook BOOLEAN DEFAULT false,
    supports_trades BOOLEAN DEFAULT true,
    supports_historical BOOLEAN DEFAULT false,
    max_symbols_per_connection INTEGER,
    rate_limit_per_second INTEGER,

    -- Parser configuration
    parser_config JSONB,

    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(feed_id, effective_from)
);

CREATE TABLE feed_calendars (
    id SERIAL PRIMARY KEY,
    feed_id INTEGER REFERENCES feeds(id) ON DELETE CASCADE,
    effective_from DATE NOT NULL,
    effective_to DATE,

    timezone VARCHAR(64),
    open_time TIME,
    close_time TIME,
    holiday_calendar VARCHAR(64),  -- 'us_equity', 'crypto_247', 'custom'

    UNIQUE(feed_id, effective_from)
);

CREATE INDEX idx_feeds_name ON feeds(name);
CREATE INDEX idx_feeds_status ON feeds(status);
CREATE INDEX idx_feed_versions_feed_id ON feed_versions(feed_id);
CREATE INDEX idx_feed_versions_effective ON feed_versions(feed_id, effective_from);
```

**Step 2: Create feeds migration (down)**

Create `internal/db/migrations/000002_feeds.down.sql`:

```sql
DROP TABLE IF EXISTS feed_calendars;
DROP TABLE IF EXISTS feed_versions;
DROP TABLE IF EXISTS feeds;
```

**Step 3: Commit**

```bash
git add internal/db/migrations/000002_feeds.*
git commit -m "feat: add feed registry schema (feeds, feed_versions, feed_calendars)"
```

---

## Task 5: Schema Registry Tables

**Files:**
- Create: `internal/db/migrations/000003_schemas.up.sql`
- Create: `internal/db/migrations/000003_schemas.down.sql`

**Step 1: Create schema registry migration (up)**

Create `internal/db/migrations/000003_schemas.up.sql`:

```sql
-- Schema Registry: tracks schema versions for normalized data

CREATE TABLE schema_versions (
    id SERIAL PRIMARY KEY,
    name VARCHAR(64) NOT NULL,  -- 'trade', 'orderbook', 'market_status'
    version VARCHAR(32) NOT NULL,

    -- Schema definition
    format VARCHAR(32) NOT NULL,  -- 'capnp', 'protobuf', 'json_schema'
    schema_definition TEXT NOT NULL,
    schema_hash VARCHAR(64) NOT NULL,  -- SHA256 for integrity

    -- Lifecycle
    status VARCHAR(16) DEFAULT 'active',  -- 'draft', 'active', 'deprecated'
    effective_from DATE NOT NULL,
    deprecated_at DATE,

    -- Compatibility
    compatible_with JSONB,  -- ['v1', 'v2']
    breaking_changes TEXT,

    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(name, version)
);

CREATE TABLE schema_migrations (
    id SERIAL PRIMARY KEY,
    from_version_id INTEGER REFERENCES schema_versions(id),
    to_version_id INTEGER REFERENCES schema_versions(id),

    migration_type VARCHAR(32),  -- 'automatic', 'manual', 'reprocess'
    migration_script TEXT,

    executed_at TIMESTAMPTZ,
    executed_by VARCHAR(64),
    status VARCHAR(16),  -- 'pending', 'running', 'completed', 'failed'

    UNIQUE(from_version_id, to_version_id)
);

CREATE INDEX idx_schema_versions_name ON schema_versions(name);
CREATE INDEX idx_schema_versions_status ON schema_versions(status);
```

**Step 2: Create schema registry migration (down)**

Create `internal/db/migrations/000003_schemas.down.sql`:

```sql
DROP TABLE IF EXISTS schema_migrations;
DROP TABLE IF EXISTS schema_versions;
```

**Step 3: Commit**

```bash
git add internal/db/migrations/000003_schemas.*
git commit -m "feat: add schema registry tables (schema_versions, schema_migrations)"
```

---

## Task 6: Environment Tables

**Files:**
- Create: `internal/db/migrations/000004_environments.up.sql`
- Create: `internal/db/migrations/000004_environments.down.sql`

**Step 1: Create environments migration (up)**

Create `internal/db/migrations/000004_environments.up.sql`:

```sql
-- Environments: versioned configuration for deployments

CREATE TABLE environments (
    id SERIAL PRIMARY KEY,
    name VARCHAR(64) UNIQUE NOT NULL,  -- 'kalshi-prod', 'kalshi-dev'
    description TEXT,
    status VARCHAR(16) DEFAULT 'active',
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE environment_versions (
    id SERIAL PRIMARY KEY,
    environment_id INTEGER REFERENCES environments(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,

    -- Configuration snapshot
    config_yaml TEXT NOT NULL,
    config_hash VARCHAR(64) NOT NULL,

    -- Deployment tracking
    deployed_at TIMESTAMPTZ,
    deployed_by VARCHAR(64),
    git_commit VARCHAR(40),

    -- Validity
    valid_from TIMESTAMPTZ,
    valid_to TIMESTAMPTZ,  -- NULL = current

    UNIQUE(environment_id, version)
);

CREATE TABLE deployment_log (
    id SERIAL PRIMARY KEY,
    environment_version_id INTEGER REFERENCES environment_versions(id),
    action VARCHAR(32) NOT NULL,  -- 'deploy', 'teardown', 'rollback'
    status VARCHAR(16) NOT NULL,  -- 'started', 'completed', 'failed'
    started_at TIMESTAMPTZ DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    error_message TEXT,

    trigger VARCHAR(32),  -- 'scheduled', 'manual', 'gitops'
    triggered_by VARCHAR(64)
);

CREATE INDEX idx_environments_name ON environments(name);
CREATE INDEX idx_environment_versions_env_id ON environment_versions(environment_id);
CREATE INDEX idx_deployment_log_env_version ON deployment_log(environment_version_id);
```

**Step 2: Create environments migration (down)**

Create `internal/db/migrations/000004_environments.down.sql`:

```sql
DROP TABLE IF EXISTS deployment_log;
DROP TABLE IF EXISTS environment_versions;
DROP TABLE IF EXISTS environments;
```

**Step 3: Commit**

```bash
git add internal/db/migrations/000004_environments.*
git commit -m "feat: add environment tables (environments, environment_versions, deployment_log)"
```

---

## Task 7: Data Inventory Tables

**Files:**
- Create: `internal/db/migrations/000005_inventory.up.sql`
- Create: `internal/db/migrations/000005_inventory.down.sql`

**Step 1: Create inventory migration (up)**

Create `internal/db/migrations/000005_inventory.up.sql`:

```sql
-- Data Inventory: tracks what data exists, where it lives, quality status

CREATE TABLE data_inventory (
    id SERIAL PRIMARY KEY,
    feed_id INTEGER REFERENCES feeds(id),
    data_type VARCHAR(32) NOT NULL,  -- 'raw', 'normalized'
    date DATE NOT NULL,

    -- Location
    storage_path TEXT NOT NULL,
    schema_version VARCHAR(32),

    -- Coverage
    symbol_count INTEGER,
    record_count BIGINT,
    byte_size BIGINT,
    first_timestamp TIMESTAMPTZ,
    last_timestamp TIMESTAMPTZ,

    -- Quality
    status VARCHAR(16) NOT NULL,  -- 'complete', 'partial', 'failed', 'processing'
    gap_count INTEGER DEFAULT 0,
    quality_score DECIMAL(3,2),  -- 0.00 to 1.00

    -- Provenance
    connector_version VARCHAR(32),
    processor_version VARCHAR(32),
    processed_at TIMESTAMPTZ,

    UNIQUE(feed_id, data_type, date, schema_version)
);

CREATE TABLE data_gaps (
    id SERIAL PRIMARY KEY,
    inventory_id INTEGER REFERENCES data_inventory(id) ON DELETE CASCADE,
    gap_start TIMESTAMPTZ NOT NULL,
    gap_end TIMESTAMPTZ NOT NULL,
    gap_type VARCHAR(32),  -- 'connection_lost', 'rate_limited', 'exchange_outage'
    resolved BOOLEAN DEFAULT false,
    resolved_at TIMESTAMPTZ,
    notes TEXT
);

CREATE TABLE data_quality_issues (
    id SERIAL PRIMARY KEY,
    inventory_id INTEGER REFERENCES data_inventory(id) ON DELETE CASCADE,
    issue_type VARCHAR(32) NOT NULL,  -- 'duplicate', 'out_of_order', 'missing_field', 'parse_error'
    severity VARCHAR(16) NOT NULL,  -- 'error', 'warning', 'info'
    count INTEGER DEFAULT 1,
    sample_data JSONB,
    detected_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_data_inventory_feed_date ON data_inventory(feed_id, date);
CREATE INDEX idx_data_inventory_status ON data_inventory(status);
CREATE INDEX idx_data_gaps_inventory ON data_gaps(inventory_id);
CREATE INDEX idx_data_quality_inventory ON data_quality_issues(inventory_id);
```

**Step 2: Create inventory migration (down)**

Create `internal/db/migrations/000005_inventory.down.sql`:

```sql
DROP TABLE IF EXISTS data_quality_issues;
DROP TABLE IF EXISTS data_gaps;
DROP TABLE IF EXISTS data_inventory;
```

**Step 3: Commit**

```bash
git add internal/db/migrations/000005_inventory.*
git commit -m "feat: add data inventory tables (data_inventory, data_gaps, data_quality_issues)"
```

---

## Task 8: Security Master Tables

**Files:**
- Create: `internal/db/migrations/000006_markets.up.sql`
- Create: `internal/db/migrations/000006_markets.down.sql`

**Step 1: Create markets migration (up)**

Create `internal/db/migrations/000006_markets.up.sql`:

```sql
-- Security Master: market/instrument metadata for each feed

CREATE TABLE markets (
    id SERIAL PRIMARY KEY,
    feed_id INTEGER REFERENCES feeds(id),
    ticker VARCHAR(64) NOT NULL,
    external_id VARCHAR(128) NOT NULL,  -- Kalshi's ID
    title TEXT NOT NULL,
    category VARCHAR(64),
    status VARCHAR(16) NOT NULL DEFAULT 'active',

    -- Contract details
    open_time TIMESTAMPTZ,
    close_time TIMESTAMPTZ,
    expiration_time TIMESTAMPTZ,
    settlement_time TIMESTAMPTZ,

    -- Settlement
    result VARCHAR(16),  -- 'yes', 'no', NULL if unsettled
    settled_at TIMESTAMPTZ,

    -- Metadata
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    raw_metadata JSONB,

    UNIQUE(feed_id, ticker)
);

CREATE TABLE market_history (
    id SERIAL PRIMARY KEY,
    market_id INTEGER REFERENCES markets(id) ON DELETE CASCADE,
    field_name VARCHAR(64) NOT NULL,
    old_value TEXT,
    new_value TEXT,
    changed_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_markets_feed_ticker ON markets(feed_id, ticker);
CREATE INDEX idx_markets_status ON markets(status);
CREATE INDEX idx_markets_expiration ON markets(expiration_time);
CREATE INDEX idx_market_history_market ON market_history(market_id);
CREATE INDEX idx_market_history_changed ON market_history(changed_at);
```

**Step 2: Create markets migration (down)**

Create `internal/db/migrations/000006_markets.down.sql`:

```sql
DROP TABLE IF EXISTS market_history;
DROP TABLE IF EXISTS markets;
```

**Step 3: Commit**

```bash
git add internal/db/migrations/000006_markets.*
git commit -m "feat: add security master tables (markets, market_history)"
```

---

## Task 9: Key Management Tables

**Files:**
- Create: `internal/db/migrations/000007_keys.up.sql`
- Create: `internal/db/migrations/000007_keys.down.sql`

**Step 1: Create keys migration (up)**

Create `internal/db/migrations/000007_keys.up.sql`:

```sql
-- Key Management: tracks API keys and credentials metadata (not values)

CREATE TABLE keys (
    id SERIAL PRIMARY KEY,
    environment_id INTEGER REFERENCES environments(id) ON DELETE CASCADE,
    name VARCHAR(64) NOT NULL,
    key_type VARCHAR(32) NOT NULL,  -- 'api_key', 'database', 'transport', 'storage'
    description TEXT,
    required BOOLEAN DEFAULT true,
    fields JSONB NOT NULL,  -- ['api_key', 'api_secret']
    rotation_days INTEGER,

    -- Status
    status VARCHAR(16) DEFAULT 'not_set',  -- 'not_set', 'set', 'expired'
    sealed_secret_ref VARCHAR(128),

    -- Audit
    last_rotated TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),

    UNIQUE(environment_id, name)
);

CREATE TABLE key_audit_log (
    id SERIAL PRIMARY KEY,
    key_id INTEGER REFERENCES keys(id) ON DELETE CASCADE,
    action VARCHAR(32) NOT NULL,  -- 'created', 'rotated', 'accessed', 'deleted'
    actor VARCHAR(64),  -- 'cli:user@host', 'system:connector'
    details JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_keys_environment ON keys(environment_id);
CREATE INDEX idx_key_audit_key_id ON key_audit_log(key_id);
CREATE INDEX idx_key_audit_created ON key_audit_log(created_at);
```

**Step 2: Create keys migration (down)**

Create `internal/db/migrations/000007_keys.down.sql`:

```sql
DROP TABLE IF EXISTS key_audit_log;
DROP TABLE IF EXISTS keys;
```

**Step 3: Commit**

```bash
git add internal/db/migrations/000007_keys.*
git commit -m "feat: add key management tables (keys, key_audit_log)"
```

---

## Task 10: Trading Day Tables

**Files:**
- Create: `internal/db/migrations/000008_trading_days.up.sql`
- Create: `internal/db/migrations/000008_trading_days.down.sql`

**Step 1: Create trading days migration (up)**

Create `internal/db/migrations/000008_trading_days.up.sql`:

```sql
-- Trading Days: first-class concept for daily operations

CREATE TABLE trading_days (
    id SERIAL PRIMARY KEY,
    environment_id INTEGER REFERENCES environments(id) ON DELETE CASCADE,
    date DATE NOT NULL,
    state VARCHAR(16) NOT NULL DEFAULT 'pending',  -- 'pending', 'starting', 'active', 'ending', 'complete', 'error', 'failed'

    -- Timing
    scheduled_start TIMESTAMPTZ,
    actual_start TIMESTAMPTZ,
    scheduled_end TIMESTAMPTZ,
    actual_end TIMESTAMPTZ,

    -- Stats
    message_count BIGINT DEFAULT 0,
    gap_count INTEGER DEFAULT 0,
    error_count INTEGER DEFAULT 0,

    -- Archive location
    raw_archive_path TEXT,
    normalized_archive_path TEXT,

    -- Workflow tracking
    start_workflow_id VARCHAR(128),
    end_workflow_id VARCHAR(128),

    -- Audit
    started_by VARCHAR(64),
    ended_by VARCHAR(64),
    notes TEXT,

    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),

    UNIQUE(environment_id, date)
);

CREATE TABLE trading_day_events (
    id SERIAL PRIMARY KEY,
    trading_day_id INTEGER REFERENCES trading_days(id) ON DELETE CASCADE,
    event_type VARCHAR(32) NOT NULL,  -- 'state_change', 'gap_detected', 'error', 'checkpoint'
    old_state VARCHAR(16),
    new_state VARCHAR(16),
    details JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_trading_days_env_date ON trading_days(environment_id, date);
CREATE INDEX idx_trading_days_state ON trading_days(state);
CREATE INDEX idx_trading_day_events_day ON trading_day_events(trading_day_id);
```

**Step 2: Create trading days migration (down)**

Create `internal/db/migrations/000008_trading_days.down.sql`:

```sql
DROP TABLE IF EXISTS trading_day_events;
DROP TABLE IF EXISTS trading_days;
```

**Step 3: Commit**

```bash
git add internal/db/migrations/000008_trading_days.*
git commit -m "feat: add trading day tables (trading_days, trading_day_events)"
```

---

## Task 11: Migrate CLI Command

**Files:**
- Create: `cmd/ssmd/migrate.go`

**Step 1: Create migrate command**

Create `cmd/ssmd/migrate.go`:

```go
package main

import (
	"fmt"

	"github.com/spf13/cobra"
	"github.com/your-org/ssmd/internal/config"
	"github.com/your-org/ssmd/internal/db"
)

var migrateCmd = &cobra.Command{
	Use:   "migrate",
	Short: "Run database migrations",
	Long:  `Apply all pending database migrations to bring the schema up to date.`,
	RunE: func(cmd *cobra.Command, args []string) error {
		cfg, err := config.Load()
		if err != nil {
			return fmt.Errorf("failed to load config: %w", err)
		}

		fmt.Println("Running migrations...")
		if err := db.RunMigrations(cfg.DatabaseURL); err != nil {
			return fmt.Errorf("migration failed: %w", err)
		}

		fmt.Println("Migrations completed successfully.")
		return nil
	},
}

func init() {
	rootCmd.AddCommand(migrateCmd)
}
```

**Step 2: Update db/migrate.go to use correct path**

Update `internal/db/migrate.go` to fix the embed path:

```go
package db

import (
	"embed"
	"fmt"

	"github.com/golang-migrate/migrate/v4"
	_ "github.com/golang-migrate/migrate/v4/database/postgres"
	"github.com/golang-migrate/migrate/v4/source/iofs"
)

//go:embed migrations/*.sql
var MigrationsFS embed.FS

func RunMigrations(databaseURL string) error {
	source, err := iofs.New(MigrationsFS, "migrations")
	if err != nil {
		return fmt.Errorf("failed to create migration source: %w", err)
	}

	m, err := migrate.NewWithSourceInstance("iofs", source, databaseURL)
	if err != nil {
		return fmt.Errorf("failed to create migrate instance: %w", err)
	}
	defer m.Close()

	if err := m.Up(); err != nil && err != migrate.ErrNoChange {
		return fmt.Errorf("failed to run migrations: %w", err)
	}

	return nil
}
```

**Step 3: Build and test**

```bash
go build -o bin/ssmd ./cmd/ssmd
./bin/ssmd migrate
```

Expected: "Running migrations..." followed by "Migrations completed successfully."

**Step 4: Verify tables exist**

```bash
psql postgres://postgres:ssmd@localhost:5432/ssmd -c "\dt"
```

Expected: Lists all tables (feeds, feed_versions, schema_versions, environments, etc.)

**Step 5: Commit**

```bash
git add cmd/ssmd/migrate.go internal/db/migrate.go
git commit -m "feat: add migrate CLI command"
```

---

## Task 12: SQLC Configuration

**Files:**
- Create: `sqlc.yaml`
- Create: `internal/db/queries/feeds.sql`

**Step 1: Create sqlc configuration**

Create `sqlc.yaml`:

```yaml
version: "2"
sql:
  - engine: "postgresql"
    queries: "internal/db/queries"
    schema: "internal/db/migrations"
    gen:
      go:
        package: "dbgen"
        out: "internal/db/dbgen"
        sql_package: "pgx/v5"
        emit_json_tags: true
        emit_empty_slices: true
        overrides:
          - db_type: "timestamptz"
            go_type: "time.Time"
          - db_type: "jsonb"
            go_type: "json.RawMessage"
            nullable: true
```

**Step 2: Create feeds queries**

Create `internal/db/queries/feeds.sql`:

```sql
-- name: ListFeeds :many
SELECT * FROM feeds
WHERE status = COALESCE(sqlc.narg('status'), status)
ORDER BY name;

-- name: GetFeedByName :one
SELECT * FROM feeds WHERE name = $1;

-- name: CreateFeed :one
INSERT INTO feeds (name, display_name, feed_type, status)
VALUES ($1, $2, $3, $4)
RETURNING *;

-- name: GetFeedVersionAsOf :one
SELECT fv.* FROM feed_versions fv
JOIN feeds f ON f.id = fv.feed_id
WHERE f.name = $1
  AND fv.effective_from <= $2
  AND (fv.effective_to IS NULL OR fv.effective_to > $2)
ORDER BY fv.effective_from DESC
LIMIT 1;

-- name: CreateFeedVersion :one
INSERT INTO feed_versions (
    feed_id, version, effective_from, effective_to,
    protocol, endpoint_template, auth_method, secret_ref,
    supports_orderbook, supports_trades, supports_historical,
    max_symbols_per_connection, rate_limit_per_second, parser_config
) VALUES (
    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14
)
RETURNING *;
```

**Step 3: Generate Go code**

```bash
sqlc generate
```

**Step 4: Verify generated files**

```bash
ls internal/db/dbgen/
```

Expected: `db.go`, `feeds.sql.go`, `models.go`, `querier.go`

**Step 5: Commit**

```bash
git add sqlc.yaml internal/db/queries/ internal/db/dbgen/
git commit -m "feat: add sqlc configuration and feed queries"
```

---

## Task 13: Feed Repository

**Files:**
- Create: `internal/repository/feed.go`
- Create: `internal/repository/feed_test.go`

**Step 1: Write failing test**

Create `internal/repository/feed_test.go`:

```go
package repository_test

import (
	"context"
	"testing"

	"github.com/your-org/ssmd/internal/repository"
)

func TestFeedRepository_Create(t *testing.T) {
	// This test will fail until we implement the repository
	repo := repository.NewFeedRepository(nil)
	if repo == nil {
		t.Fatal("expected non-nil repository")
	}
}
```

**Step 2: Run test to verify it fails**

```bash
go test ./internal/repository/... -v
```

Expected: FAIL (package doesn't exist yet)

**Step 3: Create feed repository**

Create `internal/repository/feed.go`:

```go
package repository

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/your-org/ssmd/internal/db/dbgen"
)

type Feed struct {
	ID          int32
	Name        string
	DisplayName string
	FeedType    string
	Status      string
	CreatedAt   time.Time
	UpdatedAt   time.Time
}

type FeedVersion struct {
	ID                      int32
	FeedID                  int32
	Version                 string
	EffectiveFrom           time.Time
	Protocol                string
	EndpointTemplate        string
	AuthMethod              *string
	SecretRef               *string
	SupportsOrderbook       bool
	SupportsTrades          bool
	MaxSymbolsPerConnection *int32
	RateLimitPerSecond      *int32
}

type FeedRepository struct {
	queries *dbgen.Queries
}

func NewFeedRepository(pool *pgxpool.Pool) *FeedRepository {
	return &FeedRepository{
		queries: dbgen.New(pool),
	}
}

func (r *FeedRepository) List(ctx context.Context, status *string) ([]Feed, error) {
	rows, err := r.queries.ListFeeds(ctx, status)
	if err != nil {
		return nil, fmt.Errorf("failed to list feeds: %w", err)
	}

	feeds := make([]Feed, len(rows))
	for i, row := range rows {
		feeds[i] = Feed{
			ID:          row.ID,
			Name:        row.Name,
			DisplayName: stringValue(row.DisplayName),
			FeedType:    row.FeedType,
			Status:      stringValue(row.Status),
			CreatedAt:   row.CreatedAt.Time,
			UpdatedAt:   row.UpdatedAt.Time,
		}
	}
	return feeds, nil
}

func (r *FeedRepository) GetByName(ctx context.Context, name string) (*Feed, error) {
	row, err := r.queries.GetFeedByName(ctx, name)
	if err != nil {
		return nil, fmt.Errorf("failed to get feed %s: %w", name, err)
	}

	return &Feed{
		ID:          row.ID,
		Name:        row.Name,
		DisplayName: stringValue(row.DisplayName),
		FeedType:    row.FeedType,
		Status:      stringValue(row.Status),
		CreatedAt:   row.CreatedAt.Time,
		UpdatedAt:   row.UpdatedAt.Time,
	}, nil
}

func (r *FeedRepository) Create(ctx context.Context, name, displayName, feedType string) (*Feed, error) {
	row, err := r.queries.CreateFeed(ctx, dbgen.CreateFeedParams{
		Name:        name,
		DisplayName: &displayName,
		FeedType:    feedType,
		Status:      stringPtr("active"),
	})
	if err != nil {
		return nil, fmt.Errorf("failed to create feed: %w", err)
	}

	return &Feed{
		ID:          row.ID,
		Name:        row.Name,
		DisplayName: stringValue(row.DisplayName),
		FeedType:    row.FeedType,
		Status:      stringValue(row.Status),
		CreatedAt:   row.CreatedAt.Time,
		UpdatedAt:   row.UpdatedAt.Time,
	}, nil
}

func stringValue(s *string) string {
	if s == nil {
		return ""
	}
	return *s
}

func stringPtr(s string) *string {
	return &s
}
```

**Step 4: Run test to verify it passes**

```bash
go test ./internal/repository/... -v
```

Expected: PASS

**Step 5: Commit**

```bash
git add internal/repository/
git commit -m "feat: add feed repository with list, get, create operations"
```

---

## Task 14: Feed CLI Commands

**Files:**
- Create: `cmd/ssmd/feed.go`

**Step 1: Create feed command**

Create `cmd/ssmd/feed.go`:

```go
package main

import (
	"context"
	"fmt"
	"os"
	"text/tabwriter"

	"github.com/spf13/cobra"
	"github.com/your-org/ssmd/internal/config"
	"github.com/your-org/ssmd/internal/db"
	"github.com/your-org/ssmd/internal/repository"
)

var feedCmd = &cobra.Command{
	Use:   "feed",
	Short: "Manage feed registry",
	Long:  `Create, list, and show feeds in the metadata registry.`,
}

var feedListCmd = &cobra.Command{
	Use:   "list",
	Short: "List all feeds",
	RunE: func(cmd *cobra.Command, args []string) error {
		ctx := context.Background()
		cfg, err := config.Load()
		if err != nil {
			return err
		}

		database, err := db.Connect(ctx, cfg.DatabaseURL)
		if err != nil {
			return err
		}
		defer database.Close()

		repo := repository.NewFeedRepository(database.Pool)
		feeds, err := repo.List(ctx, nil)
		if err != nil {
			return err
		}

		if len(feeds) == 0 {
			fmt.Println("No feeds registered.")
			return nil
		}

		w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
		fmt.Fprintln(w, "NAME\tTYPE\tSTATUS\tCREATED")
		for _, f := range feeds {
			fmt.Fprintf(w, "%s\t%s\t%s\t%s\n",
				f.Name, f.FeedType, f.Status, f.CreatedAt.Format("2006-01-02"))
		}
		w.Flush()
		return nil
	},
}

var feedShowCmd = &cobra.Command{
	Use:   "show [name]",
	Short: "Show feed details",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		ctx := context.Background()
		cfg, err := config.Load()
		if err != nil {
			return err
		}

		database, err := db.Connect(ctx, cfg.DatabaseURL)
		if err != nil {
			return err
		}
		defer database.Close()

		repo := repository.NewFeedRepository(database.Pool)
		feed, err := repo.GetByName(ctx, args[0])
		if err != nil {
			return fmt.Errorf("feed not found: %s", args[0])
		}

		fmt.Printf("Name:         %s\n", feed.Name)
		fmt.Printf("Display Name: %s\n", feed.DisplayName)
		fmt.Printf("Type:         %s\n", feed.FeedType)
		fmt.Printf("Status:       %s\n", feed.Status)
		fmt.Printf("Created:      %s\n", feed.CreatedAt.Format("2006-01-02 15:04:05"))
		return nil
	},
}

var feedCreateCmd = &cobra.Command{
	Use:   "create [name]",
	Short: "Create a new feed",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		ctx := context.Background()
		cfg, err := config.Load()
		if err != nil {
			return err
		}

		database, err := db.Connect(ctx, cfg.DatabaseURL)
		if err != nil {
			return err
		}
		defer database.Close()

		displayName, _ := cmd.Flags().GetString("display-name")
		feedType, _ := cmd.Flags().GetString("type")

		repo := repository.NewFeedRepository(database.Pool)
		feed, err := repo.Create(ctx, args[0], displayName, feedType)
		if err != nil {
			return err
		}

		fmt.Printf("Feed '%s' created successfully.\n", feed.Name)
		return nil
	},
}

func init() {
	rootCmd.AddCommand(feedCmd)
	feedCmd.AddCommand(feedListCmd)
	feedCmd.AddCommand(feedShowCmd)
	feedCmd.AddCommand(feedCreateCmd)

	feedCreateCmd.Flags().String("display-name", "", "Human-readable name")
	feedCreateCmd.Flags().String("type", "websocket", "Feed type (websocket, rest, multicast)")
}
```

**Step 2: Build and test**

```bash
go build -o bin/ssmd ./cmd/ssmd
./bin/ssmd feed list
```

Expected: "No feeds registered."

**Step 3: Create a test feed**

```bash
./bin/ssmd feed create kalshi --display-name "Kalshi Exchange" --type websocket
./bin/ssmd feed list
```

Expected: Table showing kalshi feed

**Step 4: Commit**

```bash
git add cmd/ssmd/feed.go
git commit -m "feat: add feed CLI commands (list, show, create)"
```

---

## Task 15: Schema CLI Commands

**Files:**
- Create: `internal/db/queries/schemas.sql`
- Create: `internal/repository/schema.go`
- Create: `cmd/ssmd/schema.go`

**Step 1: Create schema queries**

Create `internal/db/queries/schemas.sql`:

```sql
-- name: ListSchemas :many
SELECT * FROM schema_versions
WHERE status = COALESCE(sqlc.narg('status'), status)
ORDER BY name, version;

-- name: GetSchemaByNameVersion :one
SELECT * FROM schema_versions
WHERE name = $1 AND version = $2;

-- name: CreateSchema :one
INSERT INTO schema_versions (
    name, version, format, schema_definition, schema_hash,
    status, effective_from
) VALUES ($1, $2, $3, $4, $5, $6, $7)
RETURNING *;
```

**Step 2: Regenerate sqlc**

```bash
sqlc generate
```

**Step 3: Create schema repository**

Create `internal/repository/schema.go`:

```go
package repository

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgtype"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/your-org/ssmd/internal/db/dbgen"
)

type Schema struct {
	ID               int32
	Name             string
	Version          string
	Format           string
	SchemaDefinition string
	SchemaHash       string
	Status           string
	EffectiveFrom    time.Time
}

type SchemaRepository struct {
	queries *dbgen.Queries
}

func NewSchemaRepository(pool *pgxpool.Pool) *SchemaRepository {
	return &SchemaRepository{
		queries: dbgen.New(pool),
	}
}

func (r *SchemaRepository) List(ctx context.Context, status *string) ([]Schema, error) {
	rows, err := r.queries.ListSchemas(ctx, status)
	if err != nil {
		return nil, fmt.Errorf("failed to list schemas: %w", err)
	}

	schemas := make([]Schema, len(rows))
	for i, row := range rows {
		schemas[i] = Schema{
			ID:               row.ID,
			Name:             row.Name,
			Version:          row.Version,
			Format:           row.Format,
			SchemaDefinition: row.SchemaDefinition,
			SchemaHash:       row.SchemaHash,
			Status:           stringValue(row.Status),
			EffectiveFrom:    row.EffectiveFrom.Time,
		}
	}
	return schemas, nil
}

func (r *SchemaRepository) Register(ctx context.Context, name, version, format, definition string) (*Schema, error) {
	hash := sha256.Sum256([]byte(definition))
	hashStr := hex.EncodeToString(hash[:])

	row, err := r.queries.CreateSchema(ctx, dbgen.CreateSchemaParams{
		Name:             name,
		Version:          version,
		Format:           format,
		SchemaDefinition: definition,
		SchemaHash:       hashStr,
		Status:           stringPtr("active"),
		EffectiveFrom:    pgtype.Date{Time: time.Now(), Valid: true},
	})
	if err != nil {
		return nil, fmt.Errorf("failed to register schema: %w", err)
	}

	return &Schema{
		ID:               row.ID,
		Name:             row.Name,
		Version:          row.Version,
		Format:           row.Format,
		SchemaDefinition: row.SchemaDefinition,
		SchemaHash:       row.SchemaHash,
		Status:           stringValue(row.Status),
		EffectiveFrom:    row.EffectiveFrom.Time,
	}, nil
}
```

**Step 4: Create schema CLI**

Create `cmd/ssmd/schema.go`:

```go
package main

import (
	"context"
	"fmt"
	"os"
	"text/tabwriter"

	"github.com/spf13/cobra"
	"github.com/your-org/ssmd/internal/config"
	"github.com/your-org/ssmd/internal/db"
	"github.com/your-org/ssmd/internal/repository"
)

var schemaCmd = &cobra.Command{
	Use:   "schema",
	Short: "Manage schema registry",
	Long:  `Register, list, and show schemas in the registry.`,
}

var schemaListCmd = &cobra.Command{
	Use:   "list",
	Short: "List all schemas",
	RunE: func(cmd *cobra.Command, args []string) error {
		ctx := context.Background()
		cfg, err := config.Load()
		if err != nil {
			return err
		}

		database, err := db.Connect(ctx, cfg.DatabaseURL)
		if err != nil {
			return err
		}
		defer database.Close()

		repo := repository.NewSchemaRepository(database.Pool)
		schemas, err := repo.List(ctx, nil)
		if err != nil {
			return err
		}

		if len(schemas) == 0 {
			fmt.Println("No schemas registered.")
			return nil
		}

		w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
		fmt.Fprintln(w, "NAME\tVERSION\tFORMAT\tSTATUS\tEFFECTIVE")
		for _, s := range schemas {
			fmt.Fprintf(w, "%s\t%s\t%s\t%s\t%s\n",
				s.Name, s.Version, s.Format, s.Status, s.EffectiveFrom.Format("2006-01-02"))
		}
		w.Flush()
		return nil
	},
}

var schemaRegisterCmd = &cobra.Command{
	Use:   "register [name] [version]",
	Short: "Register a new schema",
	Args:  cobra.ExactArgs(2),
	RunE: func(cmd *cobra.Command, args []string) error {
		ctx := context.Background()
		cfg, err := config.Load()
		if err != nil {
			return err
		}

		database, err := db.Connect(ctx, cfg.DatabaseURL)
		if err != nil {
			return err
		}
		defer database.Close()

		format, _ := cmd.Flags().GetString("format")
		file, _ := cmd.Flags().GetString("file")

		var definition string
		if file != "" {
			data, err := os.ReadFile(file)
			if err != nil {
				return fmt.Errorf("failed to read file: %w", err)
			}
			definition = string(data)
		} else {
			definition = "-- schema definition placeholder --"
		}

		repo := repository.NewSchemaRepository(database.Pool)
		schema, err := repo.Register(ctx, args[0], args[1], format, definition)
		if err != nil {
			return err
		}

		fmt.Printf("Schema '%s:%s' registered (hash: %s...)\n",
			schema.Name, schema.Version, schema.SchemaHash[:12])
		return nil
	},
}

func init() {
	rootCmd.AddCommand(schemaCmd)
	schemaCmd.AddCommand(schemaListCmd)
	schemaCmd.AddCommand(schemaRegisterCmd)

	schemaRegisterCmd.Flags().String("format", "capnp", "Schema format (capnp, protobuf, json_schema)")
	schemaRegisterCmd.Flags().String("file", "", "Path to schema definition file")
}
```

**Step 5: Build and test**

```bash
go build -o bin/ssmd ./cmd/ssmd
./bin/ssmd schema list
./bin/ssmd schema register trade v1 --format capnp
./bin/ssmd schema list
```

Expected: Shows registered schema

**Step 6: Commit**

```bash
git add internal/db/queries/schemas.sql internal/repository/schema.go cmd/ssmd/schema.go internal/db/dbgen/
git commit -m "feat: add schema registry CLI commands (list, register)"
```

---

## Task 16: Environment Validation

**Files:**
- Create: `internal/db/queries/environments.sql`
- Create: `internal/repository/environment.go`
- Create: `internal/validator/environment.go`
- Create: `cmd/ssmd/env.go`

**Step 1: Create environment queries**

Create `internal/db/queries/environments.sql`:

```sql
-- name: GetEnvironmentByName :one
SELECT * FROM environments WHERE name = $1;

-- name: CreateEnvironment :one
INSERT INTO environments (name, description, status)
VALUES ($1, $2, $3)
RETURNING *;

-- name: ListEnvironments :many
SELECT * FROM environments ORDER BY name;
```

**Step 2: Regenerate sqlc**

```bash
sqlc generate
```

**Step 3: Create environment repository**

Create `internal/repository/environment.go`:

```go
package repository

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/your-org/ssmd/internal/db/dbgen"
)

type Environment struct {
	ID          int32
	Name        string
	Description string
	Status      string
	CreatedAt   time.Time
}

type EnvironmentRepository struct {
	queries *dbgen.Queries
}

func NewEnvironmentRepository(pool *pgxpool.Pool) *EnvironmentRepository {
	return &EnvironmentRepository{
		queries: dbgen.New(pool),
	}
}

func (r *EnvironmentRepository) GetByName(ctx context.Context, name string) (*Environment, error) {
	row, err := r.queries.GetEnvironmentByName(ctx, name)
	if err != nil {
		return nil, fmt.Errorf("environment not found: %s", name)
	}

	return &Environment{
		ID:          row.ID,
		Name:        row.Name,
		Description: stringValue(row.Description),
		Status:      stringValue(row.Status),
		CreatedAt:   row.CreatedAt.Time,
	}, nil
}

func (r *EnvironmentRepository) List(ctx context.Context) ([]Environment, error) {
	rows, err := r.queries.ListEnvironments(ctx)
	if err != nil {
		return nil, fmt.Errorf("failed to list environments: %w", err)
	}

	envs := make([]Environment, len(rows))
	for i, row := range rows {
		envs[i] = Environment{
			ID:          row.ID,
			Name:        row.Name,
			Description: stringValue(row.Description),
			Status:      stringValue(row.Status),
			CreatedAt:   row.CreatedAt.Time,
		}
	}
	return envs, nil
}
```

**Step 4: Create validator**

Create `internal/validator/environment.go`:

```go
package validator

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/your-org/ssmd/internal/repository"
	"gopkg.in/yaml.v3"
)

type ValidationResult struct {
	Valid  bool
	Errors []string
	Warns  []string
}

type EnvironmentConfig struct {
	APIVersion string `yaml:"apiVersion"`
	Kind       string `yaml:"kind"`
	Metadata   struct {
		Name string `yaml:"name"`
	} `yaml:"metadata"`
	Spec struct {
		Feed string `yaml:"feed"`
	} `yaml:"spec"`
}

type EnvironmentValidator struct {
	feedRepo *repository.FeedRepository
}

func NewEnvironmentValidator(pool *pgxpool.Pool) *EnvironmentValidator {
	return &EnvironmentValidator{
		feedRepo: repository.NewFeedRepository(pool),
	}
}

func (v *EnvironmentValidator) Validate(ctx context.Context, yamlContent []byte) (*ValidationResult, error) {
	result := &ValidationResult{Valid: true}

	var config EnvironmentConfig
	if err := yaml.Unmarshal(yamlContent, &config); err != nil {
		result.Valid = false
		result.Errors = append(result.Errors, fmt.Sprintf("Invalid YAML: %v", err))
		return result, nil
	}

	// Validate feed reference
	if config.Spec.Feed != "" {
		_, err := v.feedRepo.GetByName(ctx, config.Spec.Feed)
		if err != nil {
			result.Valid = false
			result.Errors = append(result.Errors, fmt.Sprintf("Feed '%s' not found in registry", config.Spec.Feed))
		}
	} else {
		result.Valid = false
		result.Errors = append(result.Errors, "spec.feed is required")
	}

	return result, nil
}
```

**Step 5: Create env CLI command**

Create `cmd/ssmd/env.go`:

```go
package main

import (
	"context"
	"fmt"
	"os"
	"text/tabwriter"

	"github.com/spf13/cobra"
	"github.com/your-org/ssmd/internal/config"
	"github.com/your-org/ssmd/internal/db"
	"github.com/your-org/ssmd/internal/repository"
	"github.com/your-org/ssmd/internal/validator"
)

var envCmd = &cobra.Command{
	Use:   "env",
	Short: "Manage environments",
	Long:  `Validate, list, and manage deployment environments.`,
}

var envListCmd = &cobra.Command{
	Use:   "list",
	Short: "List all environments",
	RunE: func(cmd *cobra.Command, args []string) error {
		ctx := context.Background()
		cfg, err := config.Load()
		if err != nil {
			return err
		}

		database, err := db.Connect(ctx, cfg.DatabaseURL)
		if err != nil {
			return err
		}
		defer database.Close()

		repo := repository.NewEnvironmentRepository(database.Pool)
		envs, err := repo.List(ctx)
		if err != nil {
			return err
		}

		if len(envs) == 0 {
			fmt.Println("No environments configured.")
			return nil
		}

		w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
		fmt.Fprintln(w, "NAME\tSTATUS\tCREATED")
		for _, e := range envs {
			fmt.Fprintf(w, "%s\t%s\t%s\n",
				e.Name, e.Status, e.CreatedAt.Format("2006-01-02"))
		}
		w.Flush()
		return nil
	},
}

var envValidateCmd = &cobra.Command{
	Use:   "validate [file]",
	Short: "Validate an environment definition",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		ctx := context.Background()
		cfg, err := config.Load()
		if err != nil {
			return err
		}

		database, err := db.Connect(ctx, cfg.DatabaseURL)
		if err != nil {
			return err
		}
		defer database.Close()

		content, err := os.ReadFile(args[0])
		if err != nil {
			return fmt.Errorf("failed to read file: %w", err)
		}

		v := validator.NewEnvironmentValidator(database.Pool)
		result, err := v.Validate(ctx, content)
		if err != nil {
			return err
		}

		fmt.Println("Validating environment...")
		for _, e := range result.Errors {
			fmt.Printf("  ✗ %s\n", e)
		}
		for _, w := range result.Warns {
			fmt.Printf("  ○ %s\n", w)
		}

		if result.Valid {
			fmt.Println("\n✓ Environment is valid.")
		} else {
			fmt.Println("\n✗ Validation failed.")
			os.Exit(1)
		}
		return nil
	},
}

func init() {
	rootCmd.AddCommand(envCmd)
	envCmd.AddCommand(envListCmd)
	envCmd.AddCommand(envValidateCmd)
}
```

**Step 6: Add yaml dependency**

```bash
go get gopkg.in/yaml.v3
```

**Step 7: Build and test**

```bash
go build -o bin/ssmd ./cmd/ssmd
```

Create test environment file `test-env.yaml`:

```yaml
apiVersion: ssmd/v1
kind: Environment
metadata:
  name: test
spec:
  feed: kalshi
```

Test validation:

```bash
./bin/ssmd env validate test-env.yaml
```

Expected: Shows validation result (will fail if kalshi feed doesn't exist, pass if it does)

**Step 8: Commit**

```bash
git add internal/db/queries/environments.sql internal/repository/environment.go internal/validator/ cmd/ssmd/env.go internal/db/dbgen/
git commit -m "feat: add environment validation CLI (env list, env validate)"
```

---

## Task 17: Bootstrap Kalshi Feed

**Files:**
- Create: `bootstrap/kalshi.sql`

**Step 1: Create bootstrap SQL**

Create `bootstrap/kalshi.sql`:

```sql
-- Bootstrap Kalshi feed in the registry

INSERT INTO feeds (name, display_name, feed_type, status)
VALUES ('kalshi', 'Kalshi Exchange', 'websocket', 'active')
ON CONFLICT (name) DO NOTHING;

INSERT INTO feed_versions (
    feed_id, version, effective_from,
    protocol, endpoint_template, auth_method, secret_ref,
    supports_orderbook, supports_trades, supports_historical,
    max_symbols_per_connection, rate_limit_per_second
)
SELECT
    f.id, 'v2', CURRENT_DATE,
    'wss', 'wss://api.kalshi.com/trade-api/ws/v2', 'api_key', 'sealed-secret/kalshi-creds',
    true, true, false,
    100, 10
FROM feeds f WHERE f.name = 'kalshi'
ON CONFLICT (feed_id, effective_from) DO NOTHING;

INSERT INTO feed_calendars (feed_id, effective_from, timezone, holiday_calendar)
SELECT f.id, CURRENT_DATE, 'America/New_York', 'us_equity'
FROM feeds f WHERE f.name = 'kalshi'
ON CONFLICT (feed_id, effective_from) DO NOTHING;
```

**Step 2: Run bootstrap**

```bash
psql postgres://postgres:ssmd@localhost:5432/ssmd -f bootstrap/kalshi.sql
```

**Step 3: Verify**

```bash
./bin/ssmd feed list
./bin/ssmd feed show kalshi
```

Expected: Shows Kalshi feed with details

**Step 4: Commit**

```bash
git add bootstrap/
git commit -m "feat: add Kalshi feed bootstrap data"
```

---

## Task 18: Bootstrap Cap'n Proto Schemas

**Files:**
- Create: `schemas/trade.capnp`
- Create: `schemas/orderbook.capnp`
- Create: `schemas/market_status.capnp`
- Create: `bootstrap/schemas.sh`

**Step 1: Create Cap'n Proto schema files**

Create `schemas/trade.capnp`:

```capnp
@0xabcdef1234567890;

struct Trade {
  timestamp @0 :UInt64;
  ticker @1 :Text;
  price @2 :Float64;
  size @3 :UInt32;
  side @4 :Side;
  tradeId @5 :Text;
}

enum Side {
  buy @0;
  sell @1;
}
```

Create `schemas/orderbook.capnp`:

```capnp
@0xabcdef1234567891;

struct OrderBookUpdate {
  timestamp @0 :UInt64;
  ticker @1 :Text;
  bids @2 :List(Level);
  asks @3 :List(Level);
}

struct Level {
  price @0 :Float64;
  size @1 :UInt32;
}
```

Create `schemas/market_status.capnp`:

```capnp
@0xabcdef1234567892;

struct MarketStatus {
  timestamp @0 :UInt64;
  ticker @1 :Text;
  status @2 :Status;
}

enum Status {
  open @0;
  closed @1;
  halted @2;
}
```

**Step 2: Create bootstrap script**

Create `bootstrap/schemas.sh`:

```bash
#!/bin/bash
set -e

SSMD_BIN="${SSMD_BIN:-./bin/ssmd}"

echo "Registering Cap'n Proto schemas..."

$SSMD_BIN schema register trade v1 --format capnp --file schemas/trade.capnp
$SSMD_BIN schema register orderbook v1 --format capnp --file schemas/orderbook.capnp
$SSMD_BIN schema register market_status v1 --format capnp --file schemas/market_status.capnp

echo "Done. Registered schemas:"
$SSMD_BIN schema list
```

**Step 3: Run bootstrap**

```bash
chmod +x bootstrap/schemas.sh
./bootstrap/schemas.sh
```

**Step 4: Verify**

```bash
./bin/ssmd schema list
```

Expected: Shows trade:v1, orderbook:v1, market_status:v1

**Step 5: Commit**

```bash
git add schemas/ bootstrap/schemas.sh
git commit -m "feat: add Cap'n Proto schema definitions and bootstrap script"
```

---

## Task 19: Create Initial Environment Definition

**Files:**
- Create: `environments/kalshi-dev.yaml`

**Step 1: Create environment file**

Create `environments/kalshi-dev.yaml`:

```yaml
apiVersion: ssmd/v1
kind: Environment
metadata:
  name: kalshi-dev

spec:
  keys:
    kalshi:
      type: api_key
      description: "Kalshi trading API"
      required: true
      fields:
        - api_key
        - api_secret
      rotation_days: 90

    postgres:
      type: database
      description: "ssmd metadata database"
      required: true
      fields:
        - host
        - port
        - database
        - username
        - password

    nats:
      type: transport
      description: "NATS messaging"
      required: true
      fields:
        - url
        - username
        - password

  feed: kalshi

  schedule:
    timezone: UTC
    day_start: "00:10"
    day_end: "00:00"
    auto_roll: true

  middleware:
    transport:
      type: nats
      url: nats://localhost:4222

    storage:
      type: local
      path: /var/lib/ssmd/storage

    cache:
      type: memory
      max_size: 100MB

    journal:
      type: memory
```

**Step 2: Validate environment**

```bash
./bin/ssmd env validate environments/kalshi-dev.yaml
```

Expected: "Environment is valid." (assuming kalshi feed was bootstrapped)

**Step 3: Commit**

```bash
git add environments/
git commit -m "feat: add initial kalshi-dev environment definition"
```

---

## Task 20: Final Verification

**Step 1: Run full test**

```bash
# Clean rebuild
go build -o bin/ssmd ./cmd/ssmd

# Run migrations
./bin/ssmd migrate

# Bootstrap data
psql postgres://postgres:ssmd@localhost:5432/ssmd -f bootstrap/kalshi.sql
./bootstrap/schemas.sh

# Verify all CLI commands work
./bin/ssmd feed list
./bin/ssmd feed show kalshi
./bin/ssmd schema list
./bin/ssmd env list
./bin/ssmd env validate environments/kalshi-dev.yaml
```

**Step 2: Run tests**

```bash
go test ./... -v
```

**Step 3: Commit final state**

```bash
git add -A
git commit -m "feat: complete Phase 1 - Metadata Foundation"
```

---

## Deliverables Checklist

- [ ] PostgreSQL schema with all tables (feeds, schemas, environments, inventory, markets, keys, trading_days)
- [ ] `ssmd migrate` - runs all migrations
- [ ] `ssmd feed list/show/create` - feed registry management
- [ ] `ssmd schema list/register` - schema registry
- [ ] `ssmd env list/validate` - environment validation
- [ ] Kalshi feed bootstrapped in registry
- [ ] Cap'n Proto schemas registered (trade, orderbook, market_status)
- [ ] Initial environment definition (kalshi-dev.yaml)
- [ ] All commands validate against metadata before acting

---

*Plan created: 2025-12-16*
