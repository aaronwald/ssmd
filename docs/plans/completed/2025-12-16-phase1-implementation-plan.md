# Phase 1: GitOps Metadata Foundation - Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Go CLI (`ssmd`) that manages feed, schema, and environment configuration files with git-native workflows.

**Architecture:** Pure file operations with no runtime dependencies. CLI reads/writes YAML files in `feeds/`, `schemas/`, `environments/` directories. Git is the source of truth. Validation ensures referential integrity before commits.

**Tech Stack:** Go 1.21+, Cobra (CLI), YAML (config), SHA256 (schema hashing)

---

## Task 1: Project Setup

**Files:**
- Create: `cmd/ssmd/main.go`
- Create: `go.mod`
- Create: `go.sum` (generated)
- Create: `.gitignore`

**Step 1: Initialize Go module**

Run:
```bash
cd /workspaces/ssmd && go mod init github.com/aaronwald/ssmd
```
Expected: `go.mod` created

**Step 2: Add Cobra dependency**

Run:
```bash
cd /workspaces/ssmd && go get github.com/spf13/cobra@latest
```
Expected: `go.sum` created, cobra in `go.mod`

**Step 3: Add YAML dependency**

Run:
```bash
cd /workspaces/ssmd && go get gopkg.in/yaml.v3@latest
```
Expected: yaml.v3 in `go.mod`

**Step 4: Create main.go with root command**

```go
// cmd/ssmd/main.go
package main

import (
	"fmt"
	"os"

	"github.com/spf13/cobra"
)

var rootCmd = &cobra.Command{
	Use:   "ssmd",
	Short: "Stupid Simple Market Data - configuration management",
	Long:  `ssmd manages feed, schema, and environment configuration for market data collection.`,
}

func main() {
	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
```

**Step 5: Create .gitignore**

```
# Binaries
ssmd
*.exe

# Local CLI config
.ssmd/

# Go
vendor/
```

**Step 6: Verify build**

Run:
```bash
cd /workspaces/ssmd && go build -o ssmd ./cmd/ssmd
```
Expected: `ssmd` binary created

**Step 7: Verify CLI runs**

Run:
```bash
cd /workspaces/ssmd && ./ssmd --help
```
Expected: Help text with "Stupid Simple Market Data"

**Step 8: Commit**

```bash
cd /workspaces/ssmd && git add go.mod go.sum cmd/ .gitignore && git commit -m "feat: initialize Go project with Cobra CLI"
```

---

## Task 2: Init Command

**Files:**
- Create: `internal/cmd/init.go`
- Modify: `cmd/ssmd/main.go`

**Step 1: Write failing test for init command**

Create: `internal/cmd/init_test.go`

```go
package cmd

import (
	"os"
	"path/filepath"
	"testing"
)

func TestInitCreatesDirectories(t *testing.T) {
	// Create temp directory
	tmpDir, err := os.MkdirTemp("", "ssmd-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Run init
	err = runInit(tmpDir)
	if err != nil {
		t.Fatalf("init failed: %v", err)
	}

	// Check directories exist
	expectedDirs := []string{"feeds", "schemas", "environments", ".ssmd"}
	for _, dir := range expectedDirs {
		path := filepath.Join(tmpDir, dir)
		info, err := os.Stat(path)
		if err != nil {
			t.Errorf("directory %s not created: %v", dir, err)
			continue
		}
		if !info.IsDir() {
			t.Errorf("%s is not a directory", dir)
		}
	}
}

func TestInitCreatesGitignore(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	err = runInit(tmpDir)
	if err != nil {
		t.Fatalf("init failed: %v", err)
	}

	// Check .gitignore contains .ssmd/
	gitignorePath := filepath.Join(tmpDir, ".gitignore")
	content, err := os.ReadFile(gitignorePath)
	if err != nil {
		t.Fatalf("failed to read .gitignore: %v", err)
	}
	if !contains(string(content), ".ssmd/") {
		t.Errorf(".gitignore should contain .ssmd/, got: %s", content)
	}
}

func contains(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(s) > 0 && containsHelper(s, substr))
}

func containsHelper(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
```

**Step 2: Run test to verify it fails**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/cmd/... -v
```
Expected: FAIL - `runInit` undefined

**Step 3: Implement init command**

Create: `internal/cmd/init.go`

```go
package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/spf13/cobra"
)

func NewInitCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "init",
		Short: "Initialize ssmd in current directory",
		Long:  `Creates feeds/, schemas/, environments/, and .ssmd/ directories.`,
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, err := os.Getwd()
			if err != nil {
				return fmt.Errorf("failed to get working directory: %w", err)
			}
			return runInit(cwd)
		},
	}
}

func runInit(baseDir string) error {
	// Create directories
	dirs := []string{"feeds", "schemas", "environments", ".ssmd"}
	for _, dir := range dirs {
		path := filepath.Join(baseDir, dir)
		if err := os.MkdirAll(path, 0755); err != nil {
			return fmt.Errorf("failed to create %s: %w", dir, err)
		}
	}

	// Create or update .gitignore
	gitignorePath := filepath.Join(baseDir, ".gitignore")
	if err := ensureGitignore(gitignorePath); err != nil {
		return fmt.Errorf("failed to update .gitignore: %w", err)
	}

	// Create .ssmd/config.yaml
	configPath := filepath.Join(baseDir, ".ssmd", "config.yaml")
	if _, err := os.Stat(configPath); os.IsNotExist(err) {
		if err := os.WriteFile(configPath, []byte("# ssmd local configuration\n"), 0644); err != nil {
			return fmt.Errorf("failed to create config: %w", err)
		}
	}

	fmt.Println("Initialized ssmd configuration directories:")
	fmt.Println("  feeds/")
	fmt.Println("  schemas/")
	fmt.Println("  environments/")
	fmt.Println("  .ssmd/")
	return nil
}

func ensureGitignore(path string) error {
	entry := ".ssmd/"

	// Read existing content
	content, err := os.ReadFile(path)
	if err != nil && !os.IsNotExist(err) {
		return err
	}

	// Check if entry already exists
	if strings.Contains(string(content), entry) {
		return nil
	}

	// Append entry
	f, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		return err
	}
	defer f.Close()

	// Add newline if file doesn't end with one
	if len(content) > 0 && content[len(content)-1] != '\n' {
		if _, err := f.WriteString("\n"); err != nil {
			return err
		}
	}

	_, err = f.WriteString(entry + "\n")
	return err
}
```

**Step 4: Run test to verify it passes**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/cmd/... -v
```
Expected: PASS

**Step 5: Wire init command to main**

Modify `cmd/ssmd/main.go`:

```go
package main

import (
	"fmt"
	"os"

	"github.com/aaronwald/ssmd/internal/cmd"
	"github.com/spf13/cobra"
)

var rootCmd = &cobra.Command{
	Use:   "ssmd",
	Short: "Stupid Simple Market Data - configuration management",
	Long:  `ssmd manages feed, schema, and environment configuration for market data collection.`,
}

func init() {
	rootCmd.AddCommand(cmd.NewInitCmd())
}

func main() {
	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
```

**Step 6: Build and test CLI**

Run:
```bash
cd /workspaces/ssmd && go build -o ssmd ./cmd/ssmd && ./ssmd init --help
```
Expected: Help text for init command

**Step 7: Commit**

```bash
cd /workspaces/ssmd && git add internal/ cmd/ && git commit -m "feat: add ssmd init command"
```

---

## Task 3: Types and Models

**Files:**
- Create: `internal/types/feed.go`
- Create: `internal/types/schema.go`
- Create: `internal/types/environment.go`

**Step 1: Write test for feed type**

Create: `internal/types/feed_test.go`

```go
package types

import (
	"testing"
	"time"

	"gopkg.in/yaml.v3"
)

func TestFeedUnmarshal(t *testing.T) {
	yamlContent := `
name: kalshi
display_name: Kalshi Exchange
type: websocket
status: active
versions:
  - version: v2
    effective_from: 2025-01-01
    protocol: wss
    endpoint: wss://api.kalshi.com/trade-api/ws/v2
    auth_method: api_key
    rate_limit_per_second: 10
    supports_orderbook: true
    supports_trades: true
`
	var feed Feed
	err := yaml.Unmarshal([]byte(yamlContent), &feed)
	if err != nil {
		t.Fatalf("failed to unmarshal: %v", err)
	}

	if feed.Name != "kalshi" {
		t.Errorf("expected name 'kalshi', got '%s'", feed.Name)
	}
	if feed.Type != "websocket" {
		t.Errorf("expected type 'websocket', got '%s'", feed.Type)
	}
	if len(feed.Versions) != 1 {
		t.Fatalf("expected 1 version, got %d", len(feed.Versions))
	}
	if feed.Versions[0].Version != "v2" {
		t.Errorf("expected version 'v2', got '%s'", feed.Versions[0].Version)
	}
}

func TestFeedMarshal(t *testing.T) {
	feed := Feed{
		Name:        "kalshi",
		DisplayName: "Kalshi Exchange",
		Type:        "websocket",
		Status:      "active",
		Versions: []FeedVersion{
			{
				Version:            "v2",
				EffectiveFrom:      Date{time.Date(2025, 1, 1, 0, 0, 0, 0, time.UTC)},
				Protocol:           "wss",
				Endpoint:           "wss://api.kalshi.com/trade-api/ws/v2",
				AuthMethod:         "api_key",
				RateLimitPerSecond: 10,
				SupportsOrderbook:  true,
				SupportsTrades:     true,
			},
		},
	}

	data, err := yaml.Marshal(&feed)
	if err != nil {
		t.Fatalf("failed to marshal: %v", err)
	}

	// Verify roundtrip
	var feed2 Feed
	if err := yaml.Unmarshal(data, &feed2); err != nil {
		t.Fatalf("failed to unmarshal: %v", err)
	}

	if feed2.Name != feed.Name {
		t.Errorf("roundtrip failed: name mismatch")
	}
}
```

**Step 2: Run test to verify it fails**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/types/... -v
```
Expected: FAIL - `Feed` type undefined

**Step 3: Implement feed types**

Create: `internal/types/feed.go`

```go
package types

import (
	"fmt"
	"time"
)

// Date is a date-only type for YAML serialization
type Date struct {
	time.Time
}

func (d Date) MarshalYAML() (interface{}, error) {
	return d.Format("2006-01-02"), nil
}

func (d *Date) UnmarshalYAML(unmarshal func(interface{}) error) error {
	var s string
	if err := unmarshal(&s); err != nil {
		return err
	}
	t, err := time.Parse("2006-01-02", s)
	if err != nil {
		return fmt.Errorf("invalid date format: %w", err)
	}
	d.Time = t
	return nil
}

// Feed represents a data source configuration
type Feed struct {
	Name        string        `yaml:"name"`
	DisplayName string        `yaml:"display_name,omitempty"`
	Type        string        `yaml:"type"`
	Status      string        `yaml:"status,omitempty"`
	Versions    []FeedVersion `yaml:"versions"`
	Calendar    *Calendar     `yaml:"calendar,omitempty"`
}

// FeedVersion represents a specific version of feed configuration
type FeedVersion struct {
	Version                 string            `yaml:"version"`
	EffectiveFrom           Date              `yaml:"effective_from"`
	Protocol                string            `yaml:"protocol"`
	Endpoint                string            `yaml:"endpoint"`
	AuthMethod              string            `yaml:"auth_method,omitempty"`
	RateLimitPerSecond      int               `yaml:"rate_limit_per_second,omitempty"`
	MaxSymbolsPerConnection int               `yaml:"max_symbols_per_connection,omitempty"`
	SupportsOrderbook       bool              `yaml:"supports_orderbook,omitempty"`
	SupportsTrades          bool              `yaml:"supports_trades,omitempty"`
	SupportsHistorical      bool              `yaml:"supports_historical,omitempty"`
	ParserConfig            map[string]string `yaml:"parser_config,omitempty"`
}

// Calendar represents trading schedule
type Calendar struct {
	Timezone        string `yaml:"timezone,omitempty"`
	HolidayCalendar string `yaml:"holiday_calendar,omitempty"`
	OpenTime        string `yaml:"open_time,omitempty"`
	CloseTime       string `yaml:"close_time,omitempty"`
}
```

**Step 4: Run test to verify it passes**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/types/... -v
```
Expected: PASS

**Step 5: Write test for schema type**

Create: `internal/types/schema_test.go`

```go
package types

import (
	"testing"

	"gopkg.in/yaml.v3"
)

func TestSchemaUnmarshal(t *testing.T) {
	yamlContent := `
name: trade
format: capnp
schema_file: trade.capnp
versions:
  - version: v1
    effective_from: 2025-01-01
    status: active
    hash: "sha256:abc123"
    compatible_with: []
`
	var schema Schema
	err := yaml.Unmarshal([]byte(yamlContent), &schema)
	if err != nil {
		t.Fatalf("failed to unmarshal: %v", err)
	}

	if schema.Name != "trade" {
		t.Errorf("expected name 'trade', got '%s'", schema.Name)
	}
	if schema.Format != "capnp" {
		t.Errorf("expected format 'capnp', got '%s'", schema.Format)
	}
	if len(schema.Versions) != 1 {
		t.Fatalf("expected 1 version, got %d", len(schema.Versions))
	}
	if schema.Versions[0].Status != "active" {
		t.Errorf("expected status 'active', got '%s'", schema.Versions[0].Status)
	}
}
```

**Step 6: Run test to verify it fails**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/types/... -v
```
Expected: FAIL - `Schema` type undefined

**Step 7: Implement schema types**

Create: `internal/types/schema.go`

```go
package types

// Schema represents a data schema configuration
type Schema struct {
	Name       string          `yaml:"name"`
	Format     string          `yaml:"format"`
	SchemaFile string          `yaml:"schema_file"`
	Versions   []SchemaVersion `yaml:"versions"`
}

// SchemaVersion represents a specific version of a schema
type SchemaVersion struct {
	Version         string   `yaml:"version"`
	EffectiveFrom   Date     `yaml:"effective_from"`
	Status          string   `yaml:"status"`
	Hash            string   `yaml:"hash"`
	CompatibleWith  []string `yaml:"compatible_with,omitempty"`
	BreakingChanges string   `yaml:"breaking_changes,omitempty"`
}
```

**Step 8: Run test to verify it passes**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/types/... -v
```
Expected: PASS

**Step 9: Write test for environment type**

Create: `internal/types/environment_test.go`

```go
package types

import (
	"testing"

	"gopkg.in/yaml.v3"
)

func TestEnvironmentUnmarshal(t *testing.T) {
	yamlContent := `
name: kalshi-dev
feed: kalshi
schema: trade:v1
transport:
  type: nats
  url: nats://localhost:4222
storage:
  type: local
  path: /var/lib/ssmd/data
keys:
  kalshi:
    type: api_key
    required: true
    fields: [api_key, api_secret]
    source: env
`
	var env Environment
	err := yaml.Unmarshal([]byte(yamlContent), &env)
	if err != nil {
		t.Fatalf("failed to unmarshal: %v", err)
	}

	if env.Name != "kalshi-dev" {
		t.Errorf("expected name 'kalshi-dev', got '%s'", env.Name)
	}
	if env.Feed != "kalshi" {
		t.Errorf("expected feed 'kalshi', got '%s'", env.Feed)
	}
	if env.Schema != "trade:v1" {
		t.Errorf("expected schema 'trade:v1', got '%s'", env.Schema)
	}
	if env.Transport.Type != "nats" {
		t.Errorf("expected transport type 'nats', got '%s'", env.Transport.Type)
	}
}
```

**Step 10: Run test to verify it fails**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/types/... -v
```
Expected: FAIL - `Environment` type undefined

**Step 11: Implement environment types**

Create: `internal/types/environment.go`

```go
package types

// Environment represents a deployment configuration
type Environment struct {
	Name      string            `yaml:"name"`
	Feed      string            `yaml:"feed"`
	Schema    string            `yaml:"schema"`
	Schedule  *Schedule         `yaml:"schedule,omitempty"`
	Keys      map[string]Key    `yaml:"keys,omitempty"`
	Transport TransportConfig   `yaml:"transport"`
	Storage   StorageConfig     `yaml:"storage"`
	Cache     *CacheConfig      `yaml:"cache,omitempty"`
}

// Schedule represents when to run collection
type Schedule struct {
	Timezone string `yaml:"timezone,omitempty"`
	DayStart string `yaml:"day_start,omitempty"`
	DayEnd   string `yaml:"day_end,omitempty"`
	AutoRoll bool   `yaml:"auto_roll,omitempty"`
}

// Key represents a secret reference
type Key struct {
	Type         string   `yaml:"type"`
	Required     bool     `yaml:"required,omitempty"`
	Fields       []string `yaml:"fields"`
	Source       string   `yaml:"source"`
	RotationDays int      `yaml:"rotation_days,omitempty"`
}

// TransportConfig represents message transport settings
type TransportConfig struct {
	Type string `yaml:"type"`
	URL  string `yaml:"url,omitempty"`
}

// StorageConfig represents data storage settings
type StorageConfig struct {
	Type   string `yaml:"type"`
	Path   string `yaml:"path,omitempty"`
	Bucket string `yaml:"bucket,omitempty"`
	Region string `yaml:"region,omitempty"`
}

// CacheConfig represents cache settings
type CacheConfig struct {
	Type    string `yaml:"type"`
	MaxSize string `yaml:"max_size,omitempty"`
	URL     string `yaml:"url,omitempty"`
}
```

**Step 12: Run test to verify it passes**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/types/... -v
```
Expected: PASS

**Step 13: Commit**

```bash
cd /workspaces/ssmd && git add internal/types/ && git commit -m "feat: add types for feed, schema, environment"
```

---

## Task 4: Store Package (File I/O)

**Files:**
- Create: `internal/store/store.go`
- Create: `internal/store/store_test.go`

**Step 1: Write test for store**

Create: `internal/store/store_test.go`

```go
package store

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/aaronwald/ssmd/internal/types"
)

func TestStoreSaveFeed(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-store-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create feeds directory
	feedsDir := filepath.Join(tmpDir, "feeds")
	os.MkdirAll(feedsDir, 0755)

	s := New(tmpDir)
	feed := &types.Feed{
		Name:   "kalshi",
		Type:   "websocket",
		Status: "active",
	}

	err = s.SaveFeed(feed)
	if err != nil {
		t.Fatalf("failed to save feed: %v", err)
	}

	// Verify file exists
	path := filepath.Join(feedsDir, "kalshi.yaml")
	if _, err := os.Stat(path); os.IsNotExist(err) {
		t.Errorf("feed file not created at %s", path)
	}
}

func TestStoreLoadFeed(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-store-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create feeds directory and file
	feedsDir := filepath.Join(tmpDir, "feeds")
	os.MkdirAll(feedsDir, 0755)

	content := `name: kalshi
type: websocket
status: active
versions: []
`
	os.WriteFile(filepath.Join(feedsDir, "kalshi.yaml"), []byte(content), 0644)

	s := New(tmpDir)
	feed, err := s.LoadFeed("kalshi")
	if err != nil {
		t.Fatalf("failed to load feed: %v", err)
	}

	if feed.Name != "kalshi" {
		t.Errorf("expected name 'kalshi', got '%s'", feed.Name)
	}
}

func TestStoreListFeeds(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-store-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create feeds directory and files
	feedsDir := filepath.Join(tmpDir, "feeds")
	os.MkdirAll(feedsDir, 0755)

	content := "name: %s\ntype: websocket\nstatus: active\nversions: []\n"
	os.WriteFile(filepath.Join(feedsDir, "kalshi.yaml"), []byte("name: kalshi\ntype: websocket\nstatus: active\nversions: []\n"), 0644)
	os.WriteFile(filepath.Join(feedsDir, "polymarket.yaml"), []byte("name: polymarket\ntype: websocket\nstatus: active\nversions: []\n"), 0644)
	_ = content // suppress unused warning

	s := New(tmpDir)
	feeds, err := s.ListFeeds()
	if err != nil {
		t.Fatalf("failed to list feeds: %v", err)
	}

	if len(feeds) != 2 {
		t.Errorf("expected 2 feeds, got %d", len(feeds))
	}
}
```

**Step 2: Run test to verify it fails**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/store/... -v
```
Expected: FAIL - package not found

**Step 3: Implement store**

Create: `internal/store/store.go`

```go
package store

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/aaronwald/ssmd/internal/types"
	"gopkg.in/yaml.v3"
)

// Store handles file I/O for ssmd configuration
type Store struct {
	baseDir string
}

// New creates a new Store
func New(baseDir string) *Store {
	return &Store{baseDir: baseDir}
}

// FeedsDir returns the feeds directory path
func (s *Store) FeedsDir() string {
	return filepath.Join(s.baseDir, "feeds")
}

// SchemasDir returns the schemas directory path
func (s *Store) SchemasDir() string {
	return filepath.Join(s.baseDir, "schemas")
}

// EnvironmentsDir returns the environments directory path
func (s *Store) EnvironmentsDir() string {
	return filepath.Join(s.baseDir, "environments")
}

// SaveFeed saves a feed to feeds/<name>.yaml
func (s *Store) SaveFeed(feed *types.Feed) error {
	path := filepath.Join(s.FeedsDir(), feed.Name+".yaml")
	return s.saveYAML(path, feed)
}

// LoadFeed loads a feed from feeds/<name>.yaml
func (s *Store) LoadFeed(name string) (*types.Feed, error) {
	path := filepath.Join(s.FeedsDir(), name+".yaml")
	var feed types.Feed
	if err := s.loadYAML(path, &feed); err != nil {
		return nil, err
	}
	return &feed, nil
}

// ListFeeds returns all feeds
func (s *Store) ListFeeds() ([]*types.Feed, error) {
	return listYAMLFiles[types.Feed](s.FeedsDir())
}

// FeedExists checks if a feed exists
func (s *Store) FeedExists(name string) bool {
	path := filepath.Join(s.FeedsDir(), name+".yaml")
	_, err := os.Stat(path)
	return err == nil
}

// SaveSchema saves a schema to schemas/<name>.yaml
func (s *Store) SaveSchema(schema *types.Schema) error {
	path := filepath.Join(s.SchemasDir(), schema.Name+".yaml")
	return s.saveYAML(path, schema)
}

// LoadSchema loads a schema from schemas/<name>.yaml
func (s *Store) LoadSchema(name string) (*types.Schema, error) {
	path := filepath.Join(s.SchemasDir(), name+".yaml")
	var schema types.Schema
	if err := s.loadYAML(path, &schema); err != nil {
		return nil, err
	}
	return &schema, nil
}

// ListSchemas returns all schemas
func (s *Store) ListSchemas() ([]*types.Schema, error) {
	return listYAMLFiles[types.Schema](s.SchemasDir())
}

// SchemaExists checks if a schema exists
func (s *Store) SchemaExists(name string) bool {
	path := filepath.Join(s.SchemasDir(), name+".yaml")
	_, err := os.Stat(path)
	return err == nil
}

// SaveEnvironment saves an environment to environments/<name>.yaml
func (s *Store) SaveEnvironment(env *types.Environment) error {
	path := filepath.Join(s.EnvironmentsDir(), env.Name+".yaml")
	return s.saveYAML(path, env)
}

// LoadEnvironment loads an environment from environments/<name>.yaml
func (s *Store) LoadEnvironment(name string) (*types.Environment, error) {
	path := filepath.Join(s.EnvironmentsDir(), name+".yaml")
	var env types.Environment
	if err := s.loadYAML(path, &env); err != nil {
		return nil, err
	}
	return &env, nil
}

// ListEnvironments returns all environments
func (s *Store) ListEnvironments() ([]*types.Environment, error) {
	return listYAMLFiles[types.Environment](s.EnvironmentsDir())
}

// EnvironmentExists checks if an environment exists
func (s *Store) EnvironmentExists(name string) bool {
	path := filepath.Join(s.EnvironmentsDir(), name+".yaml")
	_, err := os.Stat(path)
	return err == nil
}

func (s *Store) saveYAML(path string, v interface{}) error {
	data, err := yaml.Marshal(v)
	if err != nil {
		return fmt.Errorf("failed to marshal: %w", err)
	}
	return os.WriteFile(path, data, 0644)
}

func (s *Store) loadYAML(path string, v interface{}) error {
	data, err := os.ReadFile(path)
	if err != nil {
		return fmt.Errorf("failed to read %s: %w", path, err)
	}
	if err := yaml.Unmarshal(data, v); err != nil {
		return fmt.Errorf("failed to parse %s: %w", path, err)
	}
	return nil
}

func listYAMLFiles[T any](dir string) ([]*T, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("failed to read directory: %w", err)
	}

	var results []*T
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".yaml") {
			continue
		}

		path := filepath.Join(dir, entry.Name())
		data, err := os.ReadFile(path)
		if err != nil {
			return nil, fmt.Errorf("failed to read %s: %w", path, err)
		}

		var item T
		if err := yaml.Unmarshal(data, &item); err != nil {
			return nil, fmt.Errorf("failed to parse %s: %w", path, err)
		}
		results = append(results, &item)
	}
	return results, nil
}
```

**Step 4: Run tests to verify they pass**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/store/... -v
```
Expected: PASS

**Step 5: Commit**

```bash
cd /workspaces/ssmd && git add internal/store/ && git commit -m "feat: add store package for file I/O"
```

---

## Task 5: Feed Commands

**Files:**
- Create: `internal/cmd/feed.go`
- Create: `internal/cmd/feed_test.go`
- Modify: `cmd/ssmd/main.go`

**Step 1: Write test for feed create**

Create: `internal/cmd/feed_test.go`

```go
package cmd

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/aaronwald/ssmd/internal/store"
)

func TestFeedCreate(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-feed-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create directories
	os.MkdirAll(filepath.Join(tmpDir, "feeds"), 0755)

	s := store.New(tmpDir)
	err = runFeedCreate(s, "kalshi", "websocket", "wss://api.kalshi.com/ws/v2", "api_key")
	if err != nil {
		t.Fatalf("failed to create feed: %v", err)
	}

	// Verify feed was created
	feed, err := s.LoadFeed("kalshi")
	if err != nil {
		t.Fatalf("failed to load feed: %v", err)
	}

	if feed.Name != "kalshi" {
		t.Errorf("expected name 'kalshi', got '%s'", feed.Name)
	}
	if feed.Type != "websocket" {
		t.Errorf("expected type 'websocket', got '%s'", feed.Type)
	}
	if len(feed.Versions) != 1 {
		t.Fatalf("expected 1 version, got %d", len(feed.Versions))
	}
	if feed.Versions[0].Endpoint != "wss://api.kalshi.com/ws/v2" {
		t.Errorf("expected endpoint 'wss://api.kalshi.com/ws/v2', got '%s'", feed.Versions[0].Endpoint)
	}
}

func TestFeedCreateRejectsDuplicate(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-feed-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	os.MkdirAll(filepath.Join(tmpDir, "feeds"), 0755)

	s := store.New(tmpDir)

	// Create first time
	err = runFeedCreate(s, "kalshi", "websocket", "wss://api.kalshi.com/ws/v2", "api_key")
	if err != nil {
		t.Fatalf("failed to create feed: %v", err)
	}

	// Create second time should fail
	err = runFeedCreate(s, "kalshi", "websocket", "wss://api.kalshi.com/ws/v2", "api_key")
	if err == nil {
		t.Error("expected error for duplicate feed, got nil")
	}
}
```

**Step 2: Run test to verify it fails**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/cmd/... -v -run TestFeed
```
Expected: FAIL - `runFeedCreate` undefined

**Step 3: Implement feed commands**

Create: `internal/cmd/feed.go`

```go
package cmd

import (
	"fmt"
	"os"
	"time"

	"github.com/aaronwald/ssmd/internal/store"
	"github.com/aaronwald/ssmd/internal/types"
	"github.com/spf13/cobra"
)

func NewFeedCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "feed",
		Short: "Manage feeds",
	}

	cmd.AddCommand(newFeedListCmd())
	cmd.AddCommand(newFeedShowCmd())
	cmd.AddCommand(newFeedCreateCmd())

	return cmd
}

func newFeedListCmd() *cobra.Command {
	var status string

	cmd := &cobra.Command{
		Use:   "list",
		Short: "List all feeds",
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			s := store.New(cwd)
			return runFeedList(s, status)
		},
	}

	cmd.Flags().StringVar(&status, "status", "", "Filter by status")
	return cmd
}

func newFeedShowCmd() *cobra.Command {
	var version string

	cmd := &cobra.Command{
		Use:   "show <name>",
		Short: "Show feed details",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			s := store.New(cwd)
			return runFeedShow(s, args[0], version)
		},
	}

	cmd.Flags().StringVar(&version, "version", "", "Show specific version")
	return cmd
}

func newFeedCreateCmd() *cobra.Command {
	var (
		feedType    string
		displayName string
		endpoint    string
		authMethod  string
		rateLimit   int
	)

	cmd := &cobra.Command{
		Use:   "create <name>",
		Short: "Create a new feed",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			s := store.New(cwd)
			return runFeedCreate(s, args[0], feedType, endpoint, authMethod)
		},
	}

	cmd.Flags().StringVar(&feedType, "type", "", "Feed type: websocket, rest, multicast (required)")
	cmd.Flags().StringVar(&displayName, "display-name", "", "Human-readable name")
	cmd.Flags().StringVar(&endpoint, "endpoint", "", "Connection URL")
	cmd.Flags().StringVar(&authMethod, "auth-method", "", "Authentication method")
	cmd.Flags().IntVar(&rateLimit, "rate-limit", 0, "Requests per second")
	cmd.MarkFlagRequired("type")

	return cmd
}

func runFeedList(s *store.Store, statusFilter string) error {
	feeds, err := s.ListFeeds()
	if err != nil {
		return err
	}

	if len(feeds) == 0 {
		fmt.Println("No feeds registered.")
		return nil
	}

	fmt.Printf("%-15s %-12s %-10s %s\n", "NAME", "TYPE", "STATUS", "VERSIONS")
	for _, feed := range feeds {
		status := feed.Status
		if status == "" {
			status = "active"
		}
		if statusFilter != "" && status != statusFilter {
			continue
		}
		fmt.Printf("%-15s %-12s %-10s %d\n", feed.Name, feed.Type, status, len(feed.Versions))
	}
	return nil
}

func runFeedShow(s *store.Store, name string, version string) error {
	feed, err := s.LoadFeed(name)
	if err != nil {
		return fmt.Errorf("feed '%s' not found", name)
	}

	fmt.Printf("Name:         %s\n", feed.Name)
	if feed.DisplayName != "" {
		fmt.Printf("Display Name: %s\n", feed.DisplayName)
	}
	fmt.Printf("Type:         %s\n", feed.Type)
	status := feed.Status
	if status == "" {
		status = "active"
	}
	fmt.Printf("Status:       %s\n", status)
	fmt.Println()

	if len(feed.Versions) > 0 {
		// Find current version (latest effective)
		var current *types.FeedVersion
		now := time.Now()
		for i := range feed.Versions {
			v := &feed.Versions[i]
			if v.EffectiveFrom.Before(now) || v.EffectiveFrom.Equal(now) {
				if current == nil || v.EffectiveFrom.After(current.EffectiveFrom.Time) {
					current = v
				}
			}
		}

		if current != nil {
			fmt.Printf("Current Version: %s (effective %s)\n", current.Version, current.EffectiveFrom.Format("2006-01-02"))
			fmt.Printf("  Endpoint:    %s\n", current.Endpoint)
			if current.AuthMethod != "" {
				fmt.Printf("  Auth:        %s\n", current.AuthMethod)
			}
			if current.RateLimitPerSecond > 0 {
				fmt.Printf("  Rate Limit:  %d/sec\n", current.RateLimitPerSecond)
			}
			fmt.Printf("  Orderbook:   %s\n", boolToYesNo(current.SupportsOrderbook))
			fmt.Printf("  Trades:      %s\n", boolToYesNo(current.SupportsTrades))
		}
	}

	if feed.Calendar != nil {
		fmt.Println()
		fmt.Println("Calendar:")
		if feed.Calendar.Timezone != "" {
			fmt.Printf("  Timezone:    %s\n", feed.Calendar.Timezone)
		}
		if feed.Calendar.OpenTime != "" || feed.Calendar.CloseTime != "" {
			fmt.Printf("  Hours:       %s - %s\n", feed.Calendar.OpenTime, feed.Calendar.CloseTime)
		}
	}

	return nil
}

func runFeedCreate(s *store.Store, name, feedType, endpoint, authMethod string) error {
	// Check if feed already exists
	if s.FeedExists(name) {
		return fmt.Errorf("feed '%s' already exists", name)
	}

	// Determine protocol from type
	protocol := "wss"
	if feedType == "rest" {
		protocol = "https"
	} else if feedType == "multicast" {
		protocol = "multicast"
	}

	feed := &types.Feed{
		Name:   name,
		Type:   feedType,
		Status: "active",
		Versions: []types.FeedVersion{
			{
				Version:        "v1",
				EffectiveFrom:  types.Date{Time: time.Now()},
				Protocol:       protocol,
				Endpoint:       endpoint,
				AuthMethod:     authMethod,
				SupportsTrades: true,
			},
		},
	}

	if err := s.SaveFeed(feed); err != nil {
		return err
	}

	fmt.Printf("Created feed '%s' at feeds/%s.yaml\n", name, name)
	return nil
}

func boolToYesNo(b bool) string {
	if b {
		return "yes"
	}
	return "no"
}
```

**Step 4: Run test to verify it passes**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/cmd/... -v -run TestFeed
```
Expected: PASS

**Step 5: Wire feed command to main**

Modify `cmd/ssmd/main.go`:

```go
package main

import (
	"fmt"
	"os"

	"github.com/aaronwald/ssmd/internal/cmd"
	"github.com/spf13/cobra"
)

var rootCmd = &cobra.Command{
	Use:   "ssmd",
	Short: "Stupid Simple Market Data - configuration management",
	Long:  `ssmd manages feed, schema, and environment configuration for market data collection.`,
}

func init() {
	rootCmd.AddCommand(cmd.NewInitCmd())
	rootCmd.AddCommand(cmd.NewFeedCmd())
}

func main() {
	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
```

**Step 6: Build and test CLI**

Run:
```bash
cd /workspaces/ssmd && go build -o ssmd ./cmd/ssmd && ./ssmd feed --help
```
Expected: Help text for feed commands

**Step 7: Commit**

```bash
cd /workspaces/ssmd && git add internal/cmd/feed*.go cmd/ssmd/main.go && git commit -m "feat: add feed list, show, create commands"
```

---

## Task 6: Schema Commands

**Files:**
- Create: `internal/cmd/schema.go`
- Create: `internal/cmd/schema_test.go`
- Modify: `cmd/ssmd/main.go`

**Step 1: Write test for schema register**

Create: `internal/cmd/schema_test.go`

```go
package cmd

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/aaronwald/ssmd/internal/store"
)

func TestSchemaRegister(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-schema-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create directories
	os.MkdirAll(filepath.Join(tmpDir, "schemas"), 0755)

	// Create a schema file
	schemaContent := `@0xabcdef1234567890;
struct Trade {
  timestamp @0 :UInt64;
  ticker @1 :Text;
}
`
	schemaPath := filepath.Join(tmpDir, "schemas", "trade.capnp")
	os.WriteFile(schemaPath, []byte(schemaContent), 0644)

	s := store.New(tmpDir)
	err = runSchemaRegister(s, "trade", schemaPath, "capnp", "active")
	if err != nil {
		t.Fatalf("failed to register schema: %v", err)
	}

	// Verify schema was created
	schema, err := s.LoadSchema("trade")
	if err != nil {
		t.Fatalf("failed to load schema: %v", err)
	}

	if schema.Name != "trade" {
		t.Errorf("expected name 'trade', got '%s'", schema.Name)
	}
	if schema.Format != "capnp" {
		t.Errorf("expected format 'capnp', got '%s'", schema.Format)
	}
	if len(schema.Versions) != 1 {
		t.Fatalf("expected 1 version, got %d", len(schema.Versions))
	}
	if schema.Versions[0].Hash == "" {
		t.Error("expected hash to be set")
	}
}

func TestSchemaHash(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-schema-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	os.MkdirAll(filepath.Join(tmpDir, "schemas"), 0755)

	schemaContent := `@0xabcdef1234567890;
struct Trade {
  timestamp @0 :UInt64;
}
`
	schemaPath := filepath.Join(tmpDir, "schemas", "trade.capnp")
	os.WriteFile(schemaPath, []byte(schemaContent), 0644)

	hash, err := computeFileHash(schemaPath)
	if err != nil {
		t.Fatalf("failed to compute hash: %v", err)
	}

	if hash == "" {
		t.Error("expected non-empty hash")
	}
	if len(hash) != 71 { // "sha256:" + 64 hex chars
		t.Errorf("expected hash length 71, got %d", len(hash))
	}
}
```

**Step 2: Run test to verify it fails**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/cmd/... -v -run TestSchema
```
Expected: FAIL - `runSchemaRegister` undefined

**Step 3: Implement schema commands**

Create: `internal/cmd/schema.go`

```go
package cmd

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"

	"github.com/aaronwald/ssmd/internal/store"
	"github.com/aaronwald/ssmd/internal/types"
	"github.com/spf13/cobra"
)

func NewSchemaCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "schema",
		Short: "Manage schemas",
	}

	cmd.AddCommand(newSchemaListCmd())
	cmd.AddCommand(newSchemaShowCmd())
	cmd.AddCommand(newSchemaRegisterCmd())
	cmd.AddCommand(newSchemaHashCmd())

	return cmd
}

func newSchemaListCmd() *cobra.Command {
	var status string

	cmd := &cobra.Command{
		Use:   "list",
		Short: "List all schemas",
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			s := store.New(cwd)
			return runSchemaList(s, status)
		},
	}

	cmd.Flags().StringVar(&status, "status", "", "Filter by status")
	return cmd
}

func newSchemaShowCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "show <name>",
		Short: "Show schema details",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			s := store.New(cwd)
			return runSchemaShow(s, args[0])
		},
	}

	return cmd
}

func newSchemaRegisterCmd() *cobra.Command {
	var (
		file   string
		format string
		status string
	)

	cmd := &cobra.Command{
		Use:   "register <name>",
		Short: "Register a new schema",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			s := store.New(cwd)
			return runSchemaRegister(s, args[0], file, format, status)
		},
	}

	cmd.Flags().StringVar(&file, "file", "", "Path to schema definition file (required)")
	cmd.Flags().StringVar(&format, "format", "", "Schema format: capnp, protobuf, json_schema")
	cmd.Flags().StringVar(&status, "status", "active", "Initial status: draft, active")
	cmd.MarkFlagRequired("file")

	return cmd
}

func newSchemaHashCmd() *cobra.Command {
	var all bool

	cmd := &cobra.Command{
		Use:   "hash [name]",
		Short: "Recompute hash for a schema",
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			s := store.New(cwd)
			if all {
				return runSchemaHashAll(s)
			}
			if len(args) == 0 {
				return fmt.Errorf("schema name required (or use --all)")
			}
			return runSchemaHashOne(s, args[0])
		},
	}

	cmd.Flags().BoolVar(&all, "all", false, "Recompute hashes for all schemas")
	return cmd
}

func runSchemaList(s *store.Store, statusFilter string) error {
	schemas, err := s.ListSchemas()
	if err != nil {
		return err
	}

	if len(schemas) == 0 {
		fmt.Println("No schemas registered.")
		return nil
	}

	fmt.Printf("%-12s %-10s %-8s %-12s %s\n", "NAME", "VERSION", "FORMAT", "STATUS", "EFFECTIVE")
	for _, schema := range schemas {
		for _, v := range schema.Versions {
			if statusFilter != "" && v.Status != statusFilter {
				continue
			}
			fmt.Printf("%-12s %-10s %-8s %-12s %s\n",
				schema.Name, v.Version, schema.Format, v.Status,
				v.EffectiveFrom.Format("2006-01-02"))
		}
	}
	return nil
}

func runSchemaShow(s *store.Store, name string) error {
	schema, err := s.LoadSchema(name)
	if err != nil {
		return fmt.Errorf("schema '%s' not found", name)
	}

	fmt.Printf("Name:    %s\n", schema.Name)
	fmt.Printf("Format:  %s\n", schema.Format)
	fmt.Printf("File:    schemas/%s\n", schema.SchemaFile)
	fmt.Println()

	if len(schema.Versions) > 0 {
		fmt.Println("Versions:")
		for _, v := range schema.Versions {
			fmt.Printf("  %s (%s, %s)\n", v.Version, v.Status, v.EffectiveFrom.Format("2006-01-02"))
			fmt.Printf("    Hash: %s\n", truncateHash(v.Hash))
			if len(v.CompatibleWith) > 0 {
				fmt.Printf("    Compatible with: %v\n", v.CompatibleWith)
			}
			if v.BreakingChanges != "" {
				fmt.Printf("    Breaking changes: %s\n", v.BreakingChanges)
			}
		}
	}

	return nil
}

func runSchemaRegister(s *store.Store, name, file, format, status string) error {
	// Check if schema already exists
	if s.SchemaExists(name) {
		return fmt.Errorf("schema '%s' already exists", name)
	}

	// Infer format from file extension if not provided
	if format == "" {
		ext := filepath.Ext(file)
		switch ext {
		case ".capnp":
			format = "capnp"
		case ".proto":
			format = "protobuf"
		case ".json":
			format = "json_schema"
		default:
			return fmt.Errorf("cannot infer format from extension '%s', use --format", ext)
		}
	}

	// Compute hash
	hash, err := computeFileHash(file)
	if err != nil {
		return fmt.Errorf("failed to compute hash: %w", err)
	}

	// Determine schema file name
	schemaFileName := filepath.Base(file)

	// Copy schema file to schemas/ if not already there
	destPath := filepath.Join(s.SchemasDir(), schemaFileName)
	if file != destPath {
		if err := copyFile(file, destPath); err != nil {
			return fmt.Errorf("failed to copy schema file: %w", err)
		}
	}

	schema := &types.Schema{
		Name:       name,
		Format:     format,
		SchemaFile: schemaFileName,
		Versions: []types.SchemaVersion{
			{
				Version:       "v1",
				EffectiveFrom: types.Date{Time: time.Now()},
				Status:        status,
				Hash:          hash,
			},
		},
	}

	if err := s.SaveSchema(schema); err != nil {
		return err
	}

	fmt.Printf("Registered schema '%s' at schemas/%s.yaml\n", name, name)
	return nil
}

func runSchemaHashOne(s *store.Store, name string) error {
	schema, err := s.LoadSchema(name)
	if err != nil {
		return fmt.Errorf("schema '%s' not found", name)
	}

	schemaPath := filepath.Join(s.SchemasDir(), schema.SchemaFile)
	hash, err := computeFileHash(schemaPath)
	if err != nil {
		return fmt.Errorf("failed to compute hash: %w", err)
	}

	// Update all versions with new hash (or just latest?)
	// For now, just show the hash
	fmt.Printf("Schema: %s\n", name)
	fmt.Printf("File:   %s\n", schema.SchemaFile)
	fmt.Printf("Hash:   %s\n", hash)

	return nil
}

func runSchemaHashAll(s *store.Store) error {
	schemas, err := s.ListSchemas()
	if err != nil {
		return err
	}

	for _, schema := range schemas {
		schemaPath := filepath.Join(s.SchemasDir(), schema.SchemaFile)
		hash, err := computeFileHash(schemaPath)
		if err != nil {
			fmt.Printf("%-12s ERROR: %v\n", schema.Name, err)
			continue
		}
		fmt.Printf("%-12s %s\n", schema.Name, truncateHash(hash))
	}
	return nil
}

func computeFileHash(path string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()

	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return "", err
	}

	return "sha256:" + hex.EncodeToString(h.Sum(nil)), nil
}

func copyFile(src, dst string) error {
	srcFile, err := os.Open(src)
	if err != nil {
		return err
	}
	defer srcFile.Close()

	dstFile, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer dstFile.Close()

	_, err = io.Copy(dstFile, srcFile)
	return err
}

func truncateHash(hash string) string {
	if len(hash) > 20 {
		return hash[:20] + "..."
	}
	return hash
}
```

**Step 4: Run test to verify it passes**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/cmd/... -v -run TestSchema
```
Expected: PASS

**Step 5: Wire schema command to main**

Modify `cmd/ssmd/main.go`:

```go
package main

import (
	"fmt"
	"os"

	"github.com/aaronwald/ssmd/internal/cmd"
	"github.com/spf13/cobra"
)

var rootCmd = &cobra.Command{
	Use:   "ssmd",
	Short: "Stupid Simple Market Data - configuration management",
	Long:  `ssmd manages feed, schema, and environment configuration for market data collection.`,
}

func init() {
	rootCmd.AddCommand(cmd.NewInitCmd())
	rootCmd.AddCommand(cmd.NewFeedCmd())
	rootCmd.AddCommand(cmd.NewSchemaCmd())
}

func main() {
	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
```

**Step 6: Build and test CLI**

Run:
```bash
cd /workspaces/ssmd && go build -o ssmd ./cmd/ssmd && ./ssmd schema --help
```
Expected: Help text for schema commands

**Step 7: Commit**

```bash
cd /workspaces/ssmd && git add internal/cmd/schema*.go cmd/ssmd/main.go && git commit -m "feat: add schema list, show, register, hash commands"
```

---

## Task 7: Environment Commands

**Files:**
- Create: `internal/cmd/env.go`
- Create: `internal/cmd/env_test.go`
- Modify: `cmd/ssmd/main.go`

**Step 1: Write test for env create**

Create: `internal/cmd/env_test.go`

```go
package cmd

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/aaronwald/ssmd/internal/store"
	"github.com/aaronwald/ssmd/internal/types"
)

func TestEnvCreate(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-env-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create directories
	os.MkdirAll(filepath.Join(tmpDir, "feeds"), 0755)
	os.MkdirAll(filepath.Join(tmpDir, "schemas"), 0755)
	os.MkdirAll(filepath.Join(tmpDir, "environments"), 0755)

	s := store.New(tmpDir)

	// Create prerequisites
	feed := &types.Feed{Name: "kalshi", Type: "websocket", Status: "active"}
	s.SaveFeed(feed)

	schema := &types.Schema{
		Name:       "trade",
		Format:     "capnp",
		SchemaFile: "trade.capnp",
		Versions: []types.SchemaVersion{
			{Version: "v1", Status: "active"},
		},
	}
	s.SaveSchema(schema)

	err = runEnvCreate(s, "kalshi-dev", "kalshi", "trade:v1", "nats", "nats://localhost:4222", "local", "/var/lib/ssmd")
	if err != nil {
		t.Fatalf("failed to create env: %v", err)
	}

	// Verify env was created
	env, err := s.LoadEnvironment("kalshi-dev")
	if err != nil {
		t.Fatalf("failed to load env: %v", err)
	}

	if env.Name != "kalshi-dev" {
		t.Errorf("expected name 'kalshi-dev', got '%s'", env.Name)
	}
	if env.Feed != "kalshi" {
		t.Errorf("expected feed 'kalshi', got '%s'", env.Feed)
	}
	if env.Schema != "trade:v1" {
		t.Errorf("expected schema 'trade:v1', got '%s'", env.Schema)
	}
}

func TestEnvCreateValidatesFeedExists(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-env-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	os.MkdirAll(filepath.Join(tmpDir, "feeds"), 0755)
	os.MkdirAll(filepath.Join(tmpDir, "schemas"), 0755)
	os.MkdirAll(filepath.Join(tmpDir, "environments"), 0755)

	s := store.New(tmpDir)

	// Don't create feed - should fail
	err = runEnvCreate(s, "kalshi-dev", "kalshi", "trade:v1", "nats", "nats://localhost:4222", "local", "/var/lib/ssmd")
	if err == nil {
		t.Error("expected error for missing feed, got nil")
	}
}
```

**Step 2: Run test to verify it fails**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/cmd/... -v -run TestEnv
```
Expected: FAIL - `runEnvCreate` undefined

**Step 3: Implement env commands**

Create: `internal/cmd/env.go`

```go
package cmd

import (
	"fmt"
	"os"
	"strings"

	"github.com/aaronwald/ssmd/internal/store"
	"github.com/aaronwald/ssmd/internal/types"
	"github.com/spf13/cobra"
)

func NewEnvCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "env",
		Short: "Manage environments",
	}

	cmd.AddCommand(newEnvListCmd())
	cmd.AddCommand(newEnvShowCmd())
	cmd.AddCommand(newEnvCreateCmd())

	return cmd
}

func newEnvListCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "list",
		Short: "List all environments",
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			s := store.New(cwd)
			return runEnvList(s)
		},
	}
}

func newEnvShowCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "show <name>",
		Short: "Show environment details",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			s := store.New(cwd)
			return runEnvShow(s, args[0])
		},
	}
}

func newEnvCreateCmd() *cobra.Command {
	var (
		feed          string
		schema        string
		transportType string
		transportURL  string
		storageType   string
		storagePath   string
	)

	cmd := &cobra.Command{
		Use:   "create <name>",
		Short: "Create a new environment",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			s := store.New(cwd)
			return runEnvCreate(s, args[0], feed, schema, transportType, transportURL, storageType, storagePath)
		},
	}

	cmd.Flags().StringVar(&feed, "feed", "", "Feed reference (required)")
	cmd.Flags().StringVar(&schema, "schema", "", "Schema reference as name:version (required)")
	cmd.Flags().StringVar(&transportType, "transport.type", "nats", "Transport type: nats, mqtt, memory")
	cmd.Flags().StringVar(&transportURL, "transport.url", "", "Transport URL")
	cmd.Flags().StringVar(&storageType, "storage.type", "local", "Storage type: local, s3")
	cmd.Flags().StringVar(&storagePath, "storage.path", "", "Local storage path")
	cmd.MarkFlagRequired("feed")
	cmd.MarkFlagRequired("schema")

	return cmd
}

func runEnvList(s *store.Store) error {
	envs, err := s.ListEnvironments()
	if err != nil {
		return err
	}

	if len(envs) == 0 {
		fmt.Println("No environments configured.")
		return nil
	}

	fmt.Printf("%-15s %-12s %-15s %s\n", "NAME", "FEED", "SCHEMA", "TRANSPORT")
	for _, env := range envs {
		fmt.Printf("%-15s %-12s %-15s %s\n", env.Name, env.Feed, env.Schema, env.Transport.Type)
	}
	return nil
}

func runEnvShow(s *store.Store, name string) error {
	env, err := s.LoadEnvironment(name)
	if err != nil {
		return fmt.Errorf("environment '%s' not found", name)
	}

	fmt.Printf("Name:     %s\n", env.Name)
	fmt.Printf("Feed:     %s\n", env.Feed)
	fmt.Printf("Schema:   %s\n", env.Schema)
	fmt.Println()

	if env.Schedule != nil {
		fmt.Println("Schedule:")
		if env.Schedule.Timezone != "" {
			fmt.Printf("  Timezone:  %s\n", env.Schedule.Timezone)
		}
		if env.Schedule.DayStart != "" {
			fmt.Printf("  Start:     %s\n", env.Schedule.DayStart)
		}
		if env.Schedule.DayEnd != "" {
			fmt.Printf("  End:       %s\n", env.Schedule.DayEnd)
		}
		fmt.Printf("  Auto-roll: %s\n", boolToYesNo(env.Schedule.AutoRoll))
		fmt.Println()
	}

	if len(env.Keys) > 0 {
		fmt.Println("Keys:")
		for name, key := range env.Keys {
			required := "required"
			if !key.Required {
				required = "optional"
			}
			fmt.Printf("  %s (%s, %s)\n", name, key.Type, required)
			fmt.Printf("    Fields: %s\n", strings.Join(key.Fields, ", "))
			fmt.Printf("    Source: %s\n", key.Source)
		}
		fmt.Println()
	}

	fmt.Println("Transport:")
	fmt.Printf("  Type: %s\n", env.Transport.Type)
	if env.Transport.URL != "" {
		fmt.Printf("  URL:  %s\n", env.Transport.URL)
	}
	fmt.Println()

	fmt.Println("Storage:")
	fmt.Printf("  Type: %s\n", env.Storage.Type)
	if env.Storage.Path != "" {
		fmt.Printf("  Path: %s\n", env.Storage.Path)
	}
	if env.Storage.Bucket != "" {
		fmt.Printf("  Bucket: %s\n", env.Storage.Bucket)
	}

	return nil
}

func runEnvCreate(s *store.Store, name, feed, schema, transportType, transportURL, storageType, storagePath string) error {
	// Check if env already exists
	if s.EnvironmentExists(name) {
		return fmt.Errorf("environment '%s' already exists", name)
	}

	// Validate feed exists
	if !s.FeedExists(feed) {
		return fmt.Errorf("feed '%s' not found", feed)
	}

	// Validate schema exists
	schemaName, schemaVersion := parseSchemaRef(schema)
	schemaObj, err := s.LoadSchema(schemaName)
	if err != nil {
		return fmt.Errorf("schema '%s' not found", schemaName)
	}

	// Find version
	versionFound := false
	for _, v := range schemaObj.Versions {
		if v.Version == schemaVersion {
			if v.Status != "active" && v.Status != "" {
				return fmt.Errorf("schema version '%s' is not active (status: %s)", schema, v.Status)
			}
			versionFound = true
			break
		}
	}
	if !versionFound {
		return fmt.Errorf("schema version '%s' not found", schemaVersion)
	}

	env := &types.Environment{
		Name:   name,
		Feed:   feed,
		Schema: schema,
		Transport: types.TransportConfig{
			Type: transportType,
			URL:  transportURL,
		},
		Storage: types.StorageConfig{
			Type: storageType,
			Path: storagePath,
		},
	}

	if err := s.SaveEnvironment(env); err != nil {
		return err
	}

	fmt.Printf("Created environment '%s' at environments/%s.yaml\n", name, name)
	return nil
}

func parseSchemaRef(ref string) (name string, version string) {
	parts := strings.SplitN(ref, ":", 2)
	name = parts[0]
	if len(parts) > 1 {
		version = parts[1]
	} else {
		version = "v1"
	}
	return
}
```

**Step 4: Run test to verify it passes**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/cmd/... -v -run TestEnv
```
Expected: PASS

**Step 5: Wire env command to main**

Modify `cmd/ssmd/main.go`:

```go
package main

import (
	"fmt"
	"os"

	"github.com/aaronwald/ssmd/internal/cmd"
	"github.com/spf13/cobra"
)

var rootCmd = &cobra.Command{
	Use:   "ssmd",
	Short: "Stupid Simple Market Data - configuration management",
	Long:  `ssmd manages feed, schema, and environment configuration for market data collection.`,
}

func init() {
	rootCmd.AddCommand(cmd.NewInitCmd())
	rootCmd.AddCommand(cmd.NewFeedCmd())
	rootCmd.AddCommand(cmd.NewSchemaCmd())
	rootCmd.AddCommand(cmd.NewEnvCmd())
}

func main() {
	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
```

**Step 6: Build and test CLI**

Run:
```bash
cd /workspaces/ssmd && go build -o ssmd ./cmd/ssmd && ./ssmd env --help
```
Expected: Help text for env commands

**Step 7: Commit**

```bash
cd /workspaces/ssmd && git add internal/cmd/env*.go cmd/ssmd/main.go && git commit -m "feat: add env list, show, create commands"
```

---

## Task 8: Validate Command

**Files:**
- Create: `internal/validator/validator.go`
- Create: `internal/validator/validator_test.go`
- Create: `internal/cmd/validate.go`
- Modify: `cmd/ssmd/main.go`

**Step 1: Write test for validator**

Create: `internal/validator/validator_test.go`

```go
package validator

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/aaronwald/ssmd/internal/store"
	"github.com/aaronwald/ssmd/internal/types"
)

func setupTestDir(t *testing.T) (string, *store.Store) {
	tmpDir, err := os.MkdirTemp("", "ssmd-validate-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}

	os.MkdirAll(filepath.Join(tmpDir, "feeds"), 0755)
	os.MkdirAll(filepath.Join(tmpDir, "schemas"), 0755)
	os.MkdirAll(filepath.Join(tmpDir, "environments"), 0755)

	return tmpDir, store.New(tmpDir)
}

func TestValidateFeed(t *testing.T) {
	tmpDir, s := setupTestDir(t)
	defer os.RemoveAll(tmpDir)

	// Valid feed
	feed := &types.Feed{
		Name:   "kalshi",
		Type:   "websocket",
		Status: "active",
		Versions: []types.FeedVersion{
			{Version: "v1", Protocol: "wss", Endpoint: "wss://example.com"},
		},
	}
	s.SaveFeed(feed)

	v := New(s)
	errs := v.ValidateFeed(feed)
	if len(errs) > 0 {
		t.Errorf("expected no errors, got: %v", errs)
	}
}

func TestValidateFeedInvalidType(t *testing.T) {
	tmpDir, s := setupTestDir(t)
	defer os.RemoveAll(tmpDir)

	feed := &types.Feed{
		Name: "kalshi",
		Type: "invalid",
	}
	s.SaveFeed(feed)

	v := New(s)
	errs := v.ValidateFeed(feed)
	if len(errs) == 0 {
		t.Error("expected error for invalid type")
	}
}

func TestValidateEnvironmentMissingFeed(t *testing.T) {
	tmpDir, s := setupTestDir(t)
	defer os.RemoveAll(tmpDir)

	env := &types.Environment{
		Name:   "test-env",
		Feed:   "nonexistent",
		Schema: "trade:v1",
		Transport: types.TransportConfig{Type: "nats"},
		Storage:   types.StorageConfig{Type: "local", Path: "/tmp"},
	}
	s.SaveEnvironment(env)

	v := New(s)
	errs := v.ValidateEnvironment(env)
	if len(errs) == 0 {
		t.Error("expected error for missing feed")
	}
}

func TestValidateAll(t *testing.T) {
	tmpDir, s := setupTestDir(t)
	defer os.RemoveAll(tmpDir)

	// Create valid feed
	feed := &types.Feed{
		Name:   "kalshi",
		Type:   "websocket",
		Status: "active",
		Versions: []types.FeedVersion{
			{Version: "v1", Protocol: "wss", Endpoint: "wss://example.com"},
		},
	}
	s.SaveFeed(feed)

	// Create valid schema
	schemaContent := "@0x123; struct Trade {}"
	os.WriteFile(filepath.Join(tmpDir, "schemas", "trade.capnp"), []byte(schemaContent), 0644)

	schema := &types.Schema{
		Name:       "trade",
		Format:     "capnp",
		SchemaFile: "trade.capnp",
		Versions: []types.SchemaVersion{
			{Version: "v1", Status: "active", Hash: "sha256:test"},
		},
	}
	s.SaveSchema(schema)

	// Create valid environment
	env := &types.Environment{
		Name:      "kalshi-dev",
		Feed:      "kalshi",
		Schema:    "trade:v1",
		Transport: types.TransportConfig{Type: "nats", URL: "nats://localhost:4222"},
		Storage:   types.StorageConfig{Type: "local", Path: "/var/lib/ssmd"},
	}
	s.SaveEnvironment(env)

	v := New(s)
	result := v.ValidateAll()

	if result.HasErrors() {
		t.Errorf("expected no errors, got: %v", result.Errors)
	}
}
```

**Step 2: Run test to verify it fails**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/validator/... -v
```
Expected: FAIL - package not found

**Step 3: Implement validator**

Create: `internal/validator/validator.go`

```go
package validator

import (
	"fmt"
	"strings"

	"github.com/aaronwald/ssmd/internal/store"
	"github.com/aaronwald/ssmd/internal/types"
)

var validFeedTypes = map[string]bool{
	"websocket": true,
	"rest":      true,
	"multicast": true,
}

var validFeedStatuses = map[string]bool{
	"active":     true,
	"deprecated": true,
	"disabled":   true,
	"":           true, // default
}

var validSchemaStatuses = map[string]bool{
	"draft":      true,
	"active":     true,
	"deprecated": true,
}

var validTransportTypes = map[string]bool{
	"nats":   true,
	"mqtt":   true,
	"memory": true,
}

var validStorageTypes = map[string]bool{
	"local": true,
	"s3":    true,
}

// ValidationResult holds validation results
type ValidationResult struct {
	Errors   []ValidationError
	Warnings []ValidationError
}

// ValidationError represents a validation error
type ValidationError struct {
	File    string
	Field   string
	Message string
}

func (e ValidationError) String() string {
	if e.Field != "" {
		return fmt.Sprintf("%s: %s: %s", e.File, e.Field, e.Message)
	}
	return fmt.Sprintf("%s: %s", e.File, e.Message)
}

// HasErrors returns true if there are errors
func (r *ValidationResult) HasErrors() bool {
	return len(r.Errors) > 0
}

// Validator validates ssmd configuration
type Validator struct {
	store *store.Store
}

// New creates a new Validator
func New(s *store.Store) *Validator {
	return &Validator{store: s}
}

// ValidateAll validates all configuration files
func (v *Validator) ValidateAll() *ValidationResult {
	result := &ValidationResult{}

	// Validate feeds
	feeds, err := v.store.ListFeeds()
	if err == nil {
		for _, feed := range feeds {
			errs := v.ValidateFeed(feed)
			result.Errors = append(result.Errors, errs...)
		}
	}

	// Validate schemas
	schemas, err := v.store.ListSchemas()
	if err == nil {
		for _, schema := range schemas {
			errs := v.ValidateSchema(schema)
			result.Errors = append(result.Errors, errs...)
		}
	}

	// Validate environments
	envs, err := v.store.ListEnvironments()
	if err == nil {
		for _, env := range envs {
			errs := v.ValidateEnvironment(env)
			result.Errors = append(result.Errors, errs...)
		}
	}

	return result
}

// ValidateFeed validates a feed configuration
func (v *Validator) ValidateFeed(feed *types.Feed) []ValidationError {
	var errs []ValidationError
	file := fmt.Sprintf("feeds/%s.yaml", feed.Name)

	// Name required
	if feed.Name == "" {
		errs = append(errs, ValidationError{File: file, Field: "name", Message: "required"})
	}

	// Type must be valid
	if !validFeedTypes[feed.Type] {
		errs = append(errs, ValidationError{
			File:    file,
			Field:   "type",
			Message: fmt.Sprintf("must be one of: websocket, rest, multicast (got '%s')", feed.Type),
		})
	}

	// Status must be valid
	if !validFeedStatuses[feed.Status] {
		errs = append(errs, ValidationError{
			File:    file,
			Field:   "status",
			Message: fmt.Sprintf("must be one of: active, deprecated, disabled (got '%s')", feed.Status),
		})
	}

	// At least one version required
	if len(feed.Versions) == 0 {
		errs = append(errs, ValidationError{File: file, Field: "versions", Message: "at least one version required"})
	}

	// Validate each version
	for i, ver := range feed.Versions {
		if ver.Version == "" {
			errs = append(errs, ValidationError{
				File:    file,
				Field:   fmt.Sprintf("versions[%d].version", i),
				Message: "required",
			})
		}
		if ver.Endpoint == "" {
			errs = append(errs, ValidationError{
				File:    file,
				Field:   fmt.Sprintf("versions[%d].endpoint", i),
				Message: "required",
			})
		}
	}

	return errs
}

// ValidateSchema validates a schema configuration
func (v *Validator) ValidateSchema(schema *types.Schema) []ValidationError {
	var errs []ValidationError
	file := fmt.Sprintf("schemas/%s.yaml", schema.Name)

	if schema.Name == "" {
		errs = append(errs, ValidationError{File: file, Field: "name", Message: "required"})
	}

	if schema.SchemaFile == "" {
		errs = append(errs, ValidationError{File: file, Field: "schema_file", Message: "required"})
	}

	if len(schema.Versions) == 0 {
		errs = append(errs, ValidationError{File: file, Field: "versions", Message: "at least one version required"})
	}

	for i, ver := range schema.Versions {
		if !validSchemaStatuses[ver.Status] {
			errs = append(errs, ValidationError{
				File:    file,
				Field:   fmt.Sprintf("versions[%d].status", i),
				Message: fmt.Sprintf("must be one of: draft, active, deprecated (got '%s')", ver.Status),
			})
		}
	}

	return errs
}

// ValidateEnvironment validates an environment configuration
func (v *Validator) ValidateEnvironment(env *types.Environment) []ValidationError {
	var errs []ValidationError
	file := fmt.Sprintf("environments/%s.yaml", env.Name)

	if env.Name == "" {
		errs = append(errs, ValidationError{File: file, Field: "name", Message: "required"})
	}

	// Feed must exist
	if env.Feed == "" {
		errs = append(errs, ValidationError{File: file, Field: "feed", Message: "required"})
	} else if !v.store.FeedExists(env.Feed) {
		errs = append(errs, ValidationError{
			File:    file,
			Field:   "feed",
			Message: fmt.Sprintf("feed '%s' not found", env.Feed),
		})
	}

	// Schema must exist and version must be active
	if env.Schema == "" {
		errs = append(errs, ValidationError{File: file, Field: "schema", Message: "required"})
	} else {
		schemaName, schemaVersion := parseSchemaRef(env.Schema)
		schema, err := v.store.LoadSchema(schemaName)
		if err != nil {
			errs = append(errs, ValidationError{
				File:    file,
				Field:   "schema",
				Message: fmt.Sprintf("schema '%s' not found", schemaName),
			})
		} else {
			found := false
			for _, ver := range schema.Versions {
				if ver.Version == schemaVersion {
					found = true
					if ver.Status == "draft" {
						errs = append(errs, ValidationError{
							File:    file,
							Field:   "schema",
							Message: fmt.Sprintf("schema version '%s' is draft, must be active", env.Schema),
						})
					}
					break
				}
			}
			if !found {
				errs = append(errs, ValidationError{
					File:    file,
					Field:   "schema",
					Message: fmt.Sprintf("schema version '%s' not found", schemaVersion),
				})
			}
		}
	}

	// Transport validation
	if !validTransportTypes[env.Transport.Type] {
		errs = append(errs, ValidationError{
			File:    file,
			Field:   "transport.type",
			Message: fmt.Sprintf("must be one of: nats, mqtt, memory (got '%s')", env.Transport.Type),
		})
	}

	// Storage validation
	if !validStorageTypes[env.Storage.Type] {
		errs = append(errs, ValidationError{
			File:    file,
			Field:   "storage.type",
			Message: fmt.Sprintf("must be one of: local, s3 (got '%s')", env.Storage.Type),
		})
	}

	if env.Storage.Type == "local" && env.Storage.Path == "" {
		errs = append(errs, ValidationError{
			File:    file,
			Field:   "storage.path",
			Message: "required for local storage",
		})
	}

	if env.Storage.Type == "s3" && env.Storage.Bucket == "" {
		errs = append(errs, ValidationError{
			File:    file,
			Field:   "storage.bucket",
			Message: "required for s3 storage",
		})
	}

	return errs
}

func parseSchemaRef(ref string) (name, version string) {
	parts := strings.SplitN(ref, ":", 2)
	name = parts[0]
	if len(parts) > 1 {
		version = parts[1]
	} else {
		version = "v1"
	}
	return
}
```

**Step 4: Run test to verify it passes**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/validator/... -v
```
Expected: PASS

**Step 5: Create validate command**

Create: `internal/cmd/validate.go`

```go
package cmd

import (
	"fmt"
	"os"

	"github.com/aaronwald/ssmd/internal/store"
	"github.com/aaronwald/ssmd/internal/validator"
	"github.com/spf13/cobra"
)

func NewValidateCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "validate [path]",
		Short: "Validate configuration files",
		Long:  `Validates feeds, schemas, and environments for correctness and referential integrity.`,
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			s := store.New(cwd)
			v := validator.New(s)

			result := v.ValidateAll()

			// Print results
			feeds, _ := s.ListFeeds()
			for _, f := range feeds {
				errs := v.ValidateFeed(f)
				if len(errs) == 0 {
					fmt.Printf("feeds/%s.yaml                    ✓ valid\n", f.Name)
				} else {
					fmt.Printf("feeds/%s.yaml                    ✗ %d errors\n", f.Name, len(errs))
				}
			}

			schemas, _ := s.ListSchemas()
			for _, sc := range schemas {
				errs := v.ValidateSchema(sc)
				if len(errs) == 0 {
					fmt.Printf("schemas/%s.yaml                  ✓ valid\n", sc.Name)
				} else {
					fmt.Printf("schemas/%s.yaml                  ✗ %d errors\n", sc.Name, len(errs))
				}
			}

			envs, _ := s.ListEnvironments()
			for _, e := range envs {
				errs := v.ValidateEnvironment(e)
				if len(errs) == 0 {
					fmt.Printf("environments/%s.yaml             ✓ valid\n", e.Name)
				} else {
					fmt.Printf("environments/%s.yaml             ✗ %d errors\n", e.Name, len(errs))
				}
			}

			fmt.Println()
			if result.HasErrors() {
				fmt.Printf("Errors: %d\n", len(result.Errors))
				for _, err := range result.Errors {
					fmt.Printf("  %s\n", err)
				}
				return fmt.Errorf("validation failed")
			}

			fmt.Println("All files valid.")
			return nil
		},
	}
}
```

**Step 6: Wire validate command to main**

Modify `cmd/ssmd/main.go`:

```go
package main

import (
	"fmt"
	"os"

	"github.com/aaronwald/ssmd/internal/cmd"
	"github.com/spf13/cobra"
)

var rootCmd = &cobra.Command{
	Use:   "ssmd",
	Short: "Stupid Simple Market Data - configuration management",
	Long:  `ssmd manages feed, schema, and environment configuration for market data collection.`,
}

func init() {
	rootCmd.AddCommand(cmd.NewInitCmd())
	rootCmd.AddCommand(cmd.NewFeedCmd())
	rootCmd.AddCommand(cmd.NewSchemaCmd())
	rootCmd.AddCommand(cmd.NewEnvCmd())
	rootCmd.AddCommand(cmd.NewValidateCmd())
}

func main() {
	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
```

**Step 7: Build and test CLI**

Run:
```bash
cd /workspaces/ssmd && go build -o ssmd ./cmd/ssmd && ./ssmd validate --help
```
Expected: Help text for validate command

**Step 8: Commit**

```bash
cd /workspaces/ssmd && git add internal/validator/ internal/cmd/validate.go cmd/ssmd/main.go && git commit -m "feat: add validate command with referential integrity checks"
```

---

## Task 9: Git Commands (diff, commit)

**Files:**
- Create: `internal/cmd/git.go`
- Create: `internal/cmd/git_test.go`
- Modify: `cmd/ssmd/main.go`

**Step 1: Write test for diff command**

Create: `internal/cmd/git_test.go`

```go
package cmd

import (
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

func TestGitDiffDetectsChanges(t *testing.T) {
	// Skip if git not available
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("git not available")
	}

	tmpDir, err := os.MkdirTemp("", "ssmd-git-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Init git repo
	runGit(t, tmpDir, "init")
	runGit(t, tmpDir, "config", "user.email", "test@test.com")
	runGit(t, tmpDir, "config", "user.name", "Test")

	// Create and commit initial file
	os.MkdirAll(filepath.Join(tmpDir, "feeds"), 0755)
	os.WriteFile(filepath.Join(tmpDir, "feeds", "kalshi.yaml"), []byte("name: kalshi\n"), 0644)
	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "initial")

	// Modify file
	os.WriteFile(filepath.Join(tmpDir, "feeds", "kalshi.yaml"), []byte("name: kalshi\ntype: websocket\n"), 0644)

	// Test diff detection
	modified, added, deleted, err := getGitStatus(tmpDir)
	if err != nil {
		t.Fatalf("failed to get git status: %v", err)
	}

	if len(modified) != 1 {
		t.Errorf("expected 1 modified file, got %d", len(modified))
	}
	if len(added) != 0 {
		t.Errorf("expected 0 added files, got %d", len(added))
	}
	if len(deleted) != 0 {
		t.Errorf("expected 0 deleted files, got %d", len(deleted))
	}
}

func runGit(t *testing.T, dir string, args ...string) {
	cmd := exec.Command("git", args...)
	cmd.Dir = dir
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git %v failed: %v\n%s", args, err, out)
	}
}
```

**Step 2: Run test to verify it fails**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/cmd/... -v -run TestGit
```
Expected: FAIL - `getGitStatus` undefined

**Step 3: Implement git commands**

Create: `internal/cmd/git.go`

```go
package cmd

import (
	"fmt"
	"os"
	"os/exec"
	"strings"

	"github.com/aaronwald/ssmd/internal/store"
	"github.com/aaronwald/ssmd/internal/validator"
	"github.com/spf13/cobra"
)

func NewDiffCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "diff",
		Short: "Show uncommitted changes to ssmd files",
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			return runDiff(cwd)
		},
	}
}

func NewCommitCmd() *cobra.Command {
	var (
		message    string
		noValidate bool
	)

	cmd := &cobra.Command{
		Use:   "commit",
		Short: "Commit changes to git",
		RunE: func(cmd *cobra.Command, args []string) error {
			cwd, _ := os.Getwd()
			return runCommit(cwd, message, noValidate)
		},
	}

	cmd.Flags().StringVarP(&message, "message", "m", "", "Commit message (required)")
	cmd.Flags().BoolVar(&noValidate, "no-validate", false, "Skip validation before commit")
	cmd.MarkFlagRequired("message")

	return cmd
}

func runDiff(baseDir string) error {
	modified, added, deleted, err := getGitStatus(baseDir)
	if err != nil {
		return err
	}

	if len(modified) == 0 && len(added) == 0 && len(deleted) == 0 {
		fmt.Println("No changes to ssmd files.")
		return nil
	}

	if len(modified) > 0 {
		fmt.Println("Modified:")
		for _, f := range modified {
			fmt.Printf("  %s\n", f)
		}
	}

	if len(added) > 0 {
		fmt.Println("\nNew:")
		for _, f := range added {
			fmt.Printf("  %s\n", f)
		}
	}

	if len(deleted) > 0 {
		fmt.Println("\nDeleted:")
		for _, f := range deleted {
			fmt.Printf("  %s\n", f)
		}
	}

	return nil
}

func runCommit(baseDir string, message string, noValidate bool) error {
	// Check for changes
	modified, added, deleted, err := getGitStatus(baseDir)
	if err != nil {
		return err
	}

	if len(modified) == 0 && len(added) == 0 && len(deleted) == 0 {
		fmt.Println("No changes to commit.")
		return nil
	}

	// Validate unless skipped
	if !noValidate {
		fmt.Println("Validating...")
		s := store.New(baseDir)
		v := validator.New(s)
		result := v.ValidateAll()
		if result.HasErrors() {
			fmt.Println("Validation failed:")
			for _, err := range result.Errors {
				fmt.Printf("  %s\n", err)
			}
			return fmt.Errorf("fix validation errors or use --no-validate")
		}
		fmt.Println("Validation passed.")
	}

	// Stage ssmd files
	allFiles := append(append(modified, added...), deleted...)
	for _, f := range allFiles {
		if err := gitAdd(baseDir, f); err != nil {
			return fmt.Errorf("failed to stage %s: %w", f, err)
		}
	}

	// Commit
	if err := gitCommit(baseDir, message); err != nil {
		return fmt.Errorf("failed to commit: %w", err)
	}

	fmt.Printf("Committed %d files.\n", len(allFiles))
	return nil
}

func getGitStatus(baseDir string) (modified, added, deleted []string, err error) {
	cmd := exec.Command("git", "status", "--porcelain", "feeds/", "schemas/", "environments/")
	cmd.Dir = baseDir
	out, err := cmd.Output()
	if err != nil {
		return nil, nil, nil, fmt.Errorf("git status failed: %w", err)
	}

	for _, line := range strings.Split(string(out), "\n") {
		if len(line) < 3 {
			continue
		}
		status := line[:2]
		file := strings.TrimSpace(line[3:])

		// Filter to only ssmd directories
		if !strings.HasPrefix(file, "feeds/") &&
			!strings.HasPrefix(file, "schemas/") &&
			!strings.HasPrefix(file, "environments/") {
			continue
		}

		switch {
		case strings.Contains(status, "M"):
			modified = append(modified, file)
		case strings.Contains(status, "A"), strings.Contains(status, "?"):
			added = append(added, file)
		case strings.Contains(status, "D"):
			deleted = append(deleted, file)
		}
	}

	return
}

func gitAdd(baseDir, file string) error {
	cmd := exec.Command("git", "add", file)
	cmd.Dir = baseDir
	return cmd.Run()
}

func gitCommit(baseDir, message string) error {
	cmd := exec.Command("git", "commit", "-m", message)
	cmd.Dir = baseDir
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	return cmd.Run()
}
```

**Step 4: Run test to verify it passes**

Run:
```bash
cd /workspaces/ssmd && go test ./internal/cmd/... -v -run TestGit
```
Expected: PASS

**Step 5: Wire git commands to main**

Modify `cmd/ssmd/main.go`:

```go
package main

import (
	"fmt"
	"os"

	"github.com/aaronwald/ssmd/internal/cmd"
	"github.com/spf13/cobra"
)

var rootCmd = &cobra.Command{
	Use:   "ssmd",
	Short: "Stupid Simple Market Data - configuration management",
	Long:  `ssmd manages feed, schema, and environment configuration for market data collection.`,
}

func init() {
	rootCmd.AddCommand(cmd.NewInitCmd())
	rootCmd.AddCommand(cmd.NewFeedCmd())
	rootCmd.AddCommand(cmd.NewSchemaCmd())
	rootCmd.AddCommand(cmd.NewEnvCmd())
	rootCmd.AddCommand(cmd.NewValidateCmd())
	rootCmd.AddCommand(cmd.NewDiffCmd())
	rootCmd.AddCommand(cmd.NewCommitCmd())
}

func main() {
	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
```

**Step 6: Build and test CLI**

Run:
```bash
cd /workspaces/ssmd && go build -o ssmd ./cmd/ssmd && ./ssmd diff --help && ./ssmd commit --help
```
Expected: Help text for diff and commit commands

**Step 7: Commit**

```bash
cd /workspaces/ssmd && git add internal/cmd/git*.go cmd/ssmd/main.go && git commit -m "feat: add diff and commit commands for git workflow"
```

---

## Task 10: Bootstrap Kalshi Configuration

**Files:**
- Create: `feeds/kalshi.yaml`
- Create: `schemas/trade.capnp`
- Create: `schemas/trade.yaml`
- Create: `environments/kalshi-dev.yaml`

**Step 1: Initialize ssmd structure**

Run:
```bash
cd /workspaces/ssmd && ./ssmd init
```
Expected: Directories created

**Step 2: Create Kalshi feed**

Run:
```bash
cd /workspaces/ssmd && ./ssmd feed create kalshi \
  --type websocket \
  --display-name "Kalshi Exchange" \
  --endpoint "wss://api.kalshi.com/trade-api/ws/v2" \
  --auth-method api_key \
  --rate-limit 10 \
  --supports-orderbook \
  --supports-trades
```
Expected: Feed created

**Step 3: Create trade schema definition**

Write `schemas/trade.capnp`:

```capnp
@0x8e4a7b3c9f2d1e5a;

struct Trade {
  timestamp @0 :UInt64;    # Unix timestamp in nanoseconds
  ticker @1 :Text;         # Market ticker
  price @2 :Float64;       # Trade price (0-1 for prediction markets)
  size @3 :UInt32;         # Number of contracts
  side @4 :Side;           # Buy or sell
  tradeId @5 :Text;        # Exchange trade ID
}

enum Side {
  buy @0;
  sell @1;
}
```

**Step 4: Register trade schema**

Run:
```bash
cd /workspaces/ssmd && ./ssmd schema register trade --file schemas/trade.capnp
```
Expected: Schema registered

**Step 5: Create development environment**

Run:
```bash
cd /workspaces/ssmd && ./ssmd env create kalshi-dev \
  --feed kalshi \
  --schema trade:v1 \
  --transport.type nats \
  --transport.url "nats://localhost:4222" \
  --storage.type local \
  --storage.path "/var/lib/ssmd/data"
```
Expected: Environment created

**Step 6: Validate configuration**

Run:
```bash
cd /workspaces/ssmd && ./ssmd validate
```
Expected: All files valid

**Step 7: Review changes**

Run:
```bash
cd /workspaces/ssmd && ./ssmd diff
```
Expected: Shows new files

**Step 8: Commit bootstrap configuration**

Run:
```bash
cd /workspaces/ssmd && ./ssmd commit -m "feat: bootstrap Kalshi configuration"
```
Expected: Files committed

---

## Task 11: End-to-End Test

**Files:**
- Create: `test/e2e/cli_test.go`

**Step 1: Write e2e test**

Create: `test/e2e/cli_test.go`

```go
package e2e

import (
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

func TestCLIWorkflow(t *testing.T) {
	// Build CLI
	buildCmd := exec.Command("go", "build", "-o", "ssmd-test", "./cmd/ssmd")
	buildCmd.Dir = filepath.Join("..", "..")
	if out, err := buildCmd.CombinedOutput(); err != nil {
		t.Fatalf("failed to build: %v\n%s", err, out)
	}
	defer os.Remove(filepath.Join("..", "..", "ssmd-test"))

	cli := filepath.Join("..", "..", "ssmd-test")

	// Create temp directory
	tmpDir, err := os.MkdirTemp("", "ssmd-e2e-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Init git
	run(t, tmpDir, "git", "init")
	run(t, tmpDir, "git", "config", "user.email", "test@test.com")
	run(t, tmpDir, "git", "config", "user.name", "Test")

	// ssmd init
	run(t, tmpDir, cli, "init")

	// Verify directories
	for _, dir := range []string{"feeds", "schemas", "environments", ".ssmd"} {
		path := filepath.Join(tmpDir, dir)
		if _, err := os.Stat(path); os.IsNotExist(err) {
			t.Errorf("directory %s not created", dir)
		}
	}

	// Create feed
	run(t, tmpDir, cli, "feed", "create", "test-feed", "--type", "websocket", "--endpoint", "wss://example.com")

	// Verify feed file
	feedPath := filepath.Join(tmpDir, "feeds", "test-feed.yaml")
	if _, err := os.Stat(feedPath); os.IsNotExist(err) {
		t.Error("feed file not created")
	}

	// List feeds
	out := runOutput(t, tmpDir, cli, "feed", "list")
	if !contains(out, "test-feed") {
		t.Errorf("feed list should contain 'test-feed', got: %s", out)
	}

	// Create schema file
	schemaContent := "@0x123456789abcdef0;\nstruct Test { value @0 :UInt64; }\n"
	os.WriteFile(filepath.Join(tmpDir, "schemas", "test.capnp"), []byte(schemaContent), 0644)

	// Register schema
	run(t, tmpDir, cli, "schema", "register", "test", "--file", filepath.Join(tmpDir, "schemas", "test.capnp"))

	// List schemas
	out = runOutput(t, tmpDir, cli, "schema", "list")
	if !contains(out, "test") {
		t.Errorf("schema list should contain 'test', got: %s", out)
	}

	// Create environment
	run(t, tmpDir, cli, "env", "create", "test-env",
		"--feed", "test-feed",
		"--schema", "test:v1",
		"--transport.type", "memory",
		"--storage.type", "local",
		"--storage.path", "/tmp/test")

	// Validate
	run(t, tmpDir, cli, "validate")

	// Show diff
	out = runOutput(t, tmpDir, cli, "diff")
	if !contains(out, "feeds/test-feed.yaml") {
		t.Errorf("diff should show test-feed.yaml, got: %s", out)
	}

	// Commit
	run(t, tmpDir, cli, "commit", "-m", "test commit")

	// Verify clean state
	out = runOutput(t, tmpDir, cli, "diff")
	if !contains(out, "No changes") {
		t.Errorf("should have no changes after commit, got: %s", out)
	}
}

func run(t *testing.T, dir string, name string, args ...string) {
	cmd := exec.Command(name, args...)
	cmd.Dir = dir
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("%s %v failed: %v\n%s", name, args, err, out)
	}
}

func runOutput(t *testing.T, dir string, name string, args ...string) string {
	cmd := exec.Command(name, args...)
	cmd.Dir = dir
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("%s %v failed: %v\n%s", name, args, err, out)
	}
	return string(out)
}

func contains(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
```

**Step 2: Run e2e test**

Run:
```bash
cd /workspaces/ssmd && go test ./test/e2e/... -v
```
Expected: PASS

**Step 3: Commit**

```bash
cd /workspaces/ssmd && git add test/ && git commit -m "test: add end-to-end CLI workflow test"
```

---

## Summary

This plan implements the Phase 1 GitOps Metadata Foundation:

| Task | Description | Commands Added |
|------|-------------|----------------|
| 1 | Project setup | - |
| 2 | Init command | `ssmd init` |
| 3 | Types | - |
| 4 | Store | - |
| 5 | Feed commands | `ssmd feed list/show/create` |
| 6 | Schema commands | `ssmd schema list/show/register/hash` |
| 7 | Environment commands | `ssmd env list/show/create` |
| 8 | Validate command | `ssmd validate` |
| 9 | Git commands | `ssmd diff`, `ssmd commit` |
| 10 | Bootstrap Kalshi | - |
| 11 | E2E test | - |

**Deliverable:** A working `ssmd` CLI that manages configuration files with git-native workflows. Run `ssmd feed list` to see Kalshi. Run `ssmd validate` to check referential integrity.
