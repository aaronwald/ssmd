# Dataset Service Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `ssmd data` CLI commands to read archived market data from GCS/local storage for agent tools.

**Architecture:** The Dataset Service is a set of Go CLI commands (`ssmd data list|sample|schema|builders`) that read manifest.json files and JSONL.gz archives. Commands support `--output json` for agent consumption. Data is read from local paths or GCS via `gsutil`.

**Tech Stack:** Go, Cobra CLI, compress/gzip, encoding/json, os/exec (for gsutil)

---

## Task 1: Data Command Scaffolding

**Files:**
- Create: `internal/cmd/data.go`
- Modify: `cmd/ssmd/main.go:27` (add DataCommand)
- Test: `internal/cmd/data_test.go`

**Step 1: Write the failing test**

```go
// internal/cmd/data_test.go
package cmd

import (
	"bytes"
	"testing"
)

func TestDataListCommand(t *testing.T) {
	cmd := DataCommand()

	// Verify subcommands exist
	subcommands := cmd.Commands()
	names := make([]string, len(subcommands))
	for i, c := range subcommands {
		names[i] = c.Name()
	}

	expected := []string{"list", "sample", "schema", "builders"}
	for _, exp := range expected {
		found := false
		for _, name := range names {
			if name == exp {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("expected subcommand %q not found", exp)
		}
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/cmd -run TestDataListCommand -v`
Expected: FAIL with "undefined: DataCommand"

**Step 3: Write minimal implementation**

```go
// internal/cmd/data.go
package cmd

import (
	"github.com/spf13/cobra"
)

var dataCmd = &cobra.Command{
	Use:   "data",
	Short: "Query archived market data",
	Long:  `List, sample, and explore archived market data from local storage or GCS.`,
}

var dataListCmd = &cobra.Command{
	Use:   "list",
	Short: "List available datasets",
	RunE:  runDataList,
}

var dataSampleCmd = &cobra.Command{
	Use:   "sample <feed> <date>",
	Short: "Sample records from a dataset",
	Args:  cobra.ExactArgs(2),
	RunE:  runDataSample,
}

var dataSchemaCmd = &cobra.Command{
	Use:   "schema <feed> <message_type>",
	Short: "Show schema for a message type",
	Args:  cobra.ExactArgs(2),
	RunE:  runDataSchema,
}

var dataBuildersCmd = &cobra.Command{
	Use:   "builders",
	Short: "List available state builders",
	RunE:  runDataBuilders,
}

// Flags
var (
	dataFeed      string
	dataFrom      string
	dataTo        string
	dataTicker    string
	dataLimit     int
	dataType      string
	dataOutputJSON bool
	dataPath      string
)

func init() {
	// List flags
	dataListCmd.Flags().StringVar(&dataFeed, "feed", "", "Filter by feed name")
	dataListCmd.Flags().StringVar(&dataFrom, "from", "", "Start date (YYYY-MM-DD)")
	dataListCmd.Flags().StringVar(&dataTo, "to", "", "End date (YYYY-MM-DD)")
	dataListCmd.Flags().BoolVar(&dataOutputJSON, "output", false, "Output as JSON (use --output json)")
	dataListCmd.Flags().StringVar(&dataPath, "path", "", "Data path (default: $SSMD_DATA_PATH or gs://ssmd-archive)")

	// Sample flags
	dataSampleCmd.Flags().StringVar(&dataTicker, "ticker", "", "Filter by ticker")
	dataSampleCmd.Flags().IntVar(&dataLimit, "limit", 10, "Max records to return")
	dataSampleCmd.Flags().StringVar(&dataType, "type", "", "Message type (trade, ticker, orderbook)")
	dataSampleCmd.Flags().BoolVar(&dataOutputJSON, "output", false, "Output as JSON")
	dataSampleCmd.Flags().StringVar(&dataPath, "path", "", "Data path")

	// Schema flags
	dataSchemaCmd.Flags().BoolVar(&dataOutputJSON, "output", false, "Output as JSON")

	// Builders flags
	dataBuildersCmd.Flags().BoolVar(&dataOutputJSON, "output", false, "Output as JSON")

	// Add subcommands
	dataCmd.AddCommand(dataListCmd)
	dataCmd.AddCommand(dataSampleCmd)
	dataCmd.AddCommand(dataSchemaCmd)
	dataCmd.AddCommand(dataBuildersCmd)
}

// DataCommand returns the data command for registration
func DataCommand() *cobra.Command {
	return dataCmd
}

// Placeholder implementations
func runDataList(cmd *cobra.Command, args []string) error {
	return nil
}

func runDataSample(cmd *cobra.Command, args []string) error {
	return nil
}

func runDataSchema(cmd *cobra.Command, args []string) error {
	return nil
}

func runDataBuilders(cmd *cobra.Command, args []string) error {
	return nil
}
```

**Step 4: Register command in main.go**

```go
// cmd/ssmd/main.go - add after line 28
rootCmd.AddCommand(cmd.DataCommand())
```

**Step 5: Run test to verify it passes**

Run: `go test ./internal/cmd -run TestDataListCommand -v`
Expected: PASS

**Step 6: Commit**

```bash
git add internal/cmd/data.go internal/cmd/data_test.go cmd/ssmd/main.go
git commit -m "feat(data): scaffold ssmd data command with subcommands"
```

---

## Task 2: Manifest Types

**Files:**
- Create: `internal/types/manifest.go`
- Test: `internal/types/manifest_test.go`

**Step 1: Write the failing test**

```go
// internal/types/manifest_test.go
package types

import (
	"encoding/json"
	"testing"
	"time"
)

func TestManifestUnmarshal(t *testing.T) {
	data := `{
		"feed": "kalshi",
		"date": "2025-12-25",
		"format": "jsonl",
		"rotation_interval": "5m",
		"files": [
			{
				"name": "1738.jsonl.gz",
				"start": "2025-12-25T17:38:00Z",
				"end": "2025-12-25T17:43:00Z",
				"records": 150,
				"bytes": 12345,
				"nats_start_seq": 100,
				"nats_end_seq": 249
			}
		],
		"gaps": [],
		"tickers": ["INXD-25001", "KXBTC-25001"],
		"message_types": ["trade", "ticker"],
		"has_gaps": false
	}`

	var m Manifest
	err := json.Unmarshal([]byte(data), &m)
	if err != nil {
		t.Fatalf("failed to unmarshal: %v", err)
	}

	if m.Feed != "kalshi" {
		t.Errorf("expected feed kalshi, got %s", m.Feed)
	}
	if m.Date != "2025-12-25" {
		t.Errorf("expected date 2025-12-25, got %s", m.Date)
	}
	if len(m.Files) != 1 {
		t.Errorf("expected 1 file, got %d", len(m.Files))
	}
	if m.Files[0].Records != 150 {
		t.Errorf("expected 150 records, got %d", m.Files[0].Records)
	}
	if len(m.Tickers) != 2 {
		t.Errorf("expected 2 tickers, got %d", len(m.Tickers))
	}
}

func TestManifestTotalRecords(t *testing.T) {
	m := Manifest{
		Files: []FileEntry{
			{Records: 100},
			{Records: 200},
			{Records: 50},
		},
	}

	if m.TotalRecords() != 350 {
		t.Errorf("expected 350 total records, got %d", m.TotalRecords())
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/types -run TestManifest -v`
Expected: FAIL with "undefined: Manifest"

**Step 3: Write minimal implementation**

```go
// internal/types/manifest.go
package types

import "time"

// Manifest represents archived data metadata for a feed/date
type Manifest struct {
	Feed             string       `json:"feed" yaml:"feed"`
	Date             string       `json:"date" yaml:"date"`
	Format           string       `json:"format" yaml:"format"`
	RotationInterval string       `json:"rotation_interval" yaml:"rotation_interval"`
	Files            []FileEntry  `json:"files" yaml:"files"`
	Gaps             []Gap        `json:"gaps" yaml:"gaps"`
	Tickers          []string     `json:"tickers" yaml:"tickers"`
	MessageTypes     []string     `json:"message_types" yaml:"message_types"`
	HasGaps          bool         `json:"has_gaps" yaml:"has_gaps"`
}

// FileEntry represents a single archived file
type FileEntry struct {
	Name         string    `json:"name" yaml:"name"`
	Start        time.Time `json:"start" yaml:"start"`
	End          time.Time `json:"end" yaml:"end"`
	Records      uint64    `json:"records" yaml:"records"`
	Bytes        uint64    `json:"bytes" yaml:"bytes"`
	NatsStartSeq uint64    `json:"nats_start_seq" yaml:"nats_start_seq"`
	NatsEndSeq   uint64    `json:"nats_end_seq" yaml:"nats_end_seq"`
}

// Gap represents a detected gap in the data stream
type Gap struct {
	AfterSeq     uint64    `json:"after_seq" yaml:"after_seq"`
	MissingCount uint64    `json:"missing_count" yaml:"missing_count"`
	DetectedAt   time.Time `json:"detected_at" yaml:"detected_at"`
}

// TotalRecords returns the sum of records across all files
func (m *Manifest) TotalRecords() uint64 {
	var total uint64
	for _, f := range m.Files {
		total += f.Records
	}
	return total
}

// TotalBytes returns the sum of bytes across all files
func (m *Manifest) TotalBytes() uint64 {
	var total uint64
	for _, f := range m.Files {
		total += f.Bytes
	}
	return total
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/types -run TestManifest -v`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/types/manifest.go internal/types/manifest_test.go
git commit -m "feat(types): add Manifest types for archived data"
```

---

## Task 3: Data Storage Abstraction

**Files:**
- Create: `internal/data/storage.go`
- Test: `internal/data/storage_test.go`

**Step 1: Write the failing test**

```go
// internal/data/storage_test.go
package data

import (
	"os"
	"path/filepath"
	"testing"
)

func TestLocalStorageListFeeds(t *testing.T) {
	// Create temp directory with test structure
	tmp := t.TempDir()

	// Create feed directories
	os.MkdirAll(filepath.Join(tmp, "kalshi", "2025-12-25"), 0755)
	os.MkdirAll(filepath.Join(tmp, "polymarket", "2025-12-24"), 0755)

	// Create manifest files
	manifest := `{"feed":"kalshi","date":"2025-12-25","files":[],"tickers":[],"message_types":[]}`
	os.WriteFile(filepath.Join(tmp, "kalshi", "2025-12-25", "manifest.json"), []byte(manifest), 0644)

	storage := NewLocalStorage(tmp)
	feeds, err := storage.ListFeeds()
	if err != nil {
		t.Fatalf("ListFeeds failed: %v", err)
	}

	if len(feeds) != 2 {
		t.Errorf("expected 2 feeds, got %d", len(feeds))
	}
}

func TestLocalStorageListDates(t *testing.T) {
	tmp := t.TempDir()

	os.MkdirAll(filepath.Join(tmp, "kalshi", "2025-12-25"), 0755)
	os.MkdirAll(filepath.Join(tmp, "kalshi", "2025-12-24"), 0755)
	os.MkdirAll(filepath.Join(tmp, "kalshi", "2025-12-23"), 0755)

	storage := NewLocalStorage(tmp)
	dates, err := storage.ListDates("kalshi")
	if err != nil {
		t.Fatalf("ListDates failed: %v", err)
	}

	if len(dates) != 3 {
		t.Errorf("expected 3 dates, got %d", len(dates))
	}
}

func TestLocalStorageGetManifest(t *testing.T) {
	tmp := t.TempDir()

	os.MkdirAll(filepath.Join(tmp, "kalshi", "2025-12-25"), 0755)
	manifest := `{"feed":"kalshi","date":"2025-12-25","format":"jsonl","files":[{"name":"1200.jsonl.gz","records":100,"bytes":5000,"start":"2025-12-25T12:00:00Z","end":"2025-12-25T12:05:00Z","nats_start_seq":1,"nats_end_seq":100}],"tickers":["INXD"],"message_types":["trade"],"has_gaps":false}`
	os.WriteFile(filepath.Join(tmp, "kalshi", "2025-12-25", "manifest.json"), []byte(manifest), 0644)

	storage := NewLocalStorage(tmp)
	m, err := storage.GetManifest("kalshi", "2025-12-25")
	if err != nil {
		t.Fatalf("GetManifest failed: %v", err)
	}

	if m.Feed != "kalshi" {
		t.Errorf("expected feed kalshi, got %s", m.Feed)
	}
	if len(m.Tickers) != 1 {
		t.Errorf("expected 1 ticker, got %d", len(m.Tickers))
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/data -run TestLocalStorage -v`
Expected: FAIL with "undefined: NewLocalStorage"

**Step 3: Write minimal implementation**

```go
// internal/data/storage.go
package data

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"

	"github.com/aaronwald/ssmd/internal/types"
)

// Storage defines the interface for accessing archived data
type Storage interface {
	ListFeeds() ([]string, error)
	ListDates(feed string) ([]string, error)
	GetManifest(feed, date string) (*types.Manifest, error)
	ReadFile(feed, date, filename string) ([]byte, error)
}

// LocalStorage implements Storage for local filesystem
type LocalStorage struct {
	basePath string
}

// NewLocalStorage creates a new local storage instance
func NewLocalStorage(basePath string) *LocalStorage {
	return &LocalStorage{basePath: basePath}
}

// ListFeeds returns all feed directories
func (s *LocalStorage) ListFeeds() ([]string, error) {
	entries, err := os.ReadDir(s.basePath)
	if err != nil {
		return nil, fmt.Errorf("reading base path: %w", err)
	}

	var feeds []string
	for _, e := range entries {
		if e.IsDir() {
			feeds = append(feeds, e.Name())
		}
	}
	sort.Strings(feeds)
	return feeds, nil
}

// ListDates returns all date directories for a feed
func (s *LocalStorage) ListDates(feed string) ([]string, error) {
	feedPath := filepath.Join(s.basePath, feed)
	entries, err := os.ReadDir(feedPath)
	if err != nil {
		return nil, fmt.Errorf("reading feed path: %w", err)
	}

	var dates []string
	for _, e := range entries {
		if e.IsDir() {
			dates = append(dates, e.Name())
		}
	}
	sort.Strings(dates)
	return dates, nil
}

// GetManifest reads and parses a manifest.json file
func (s *LocalStorage) GetManifest(feed, date string) (*types.Manifest, error) {
	manifestPath := filepath.Join(s.basePath, feed, date, "manifest.json")
	data, err := os.ReadFile(manifestPath)
	if err != nil {
		return nil, fmt.Errorf("reading manifest: %w", err)
	}

	var m types.Manifest
	if err := json.Unmarshal(data, &m); err != nil {
		return nil, fmt.Errorf("parsing manifest: %w", err)
	}
	return &m, nil
}

// ReadFile reads raw file contents
func (s *LocalStorage) ReadFile(feed, date, filename string) ([]byte, error) {
	filePath := filepath.Join(s.basePath, feed, date, filename)
	return os.ReadFile(filePath)
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/data -run TestLocalStorage -v`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/data/storage.go internal/data/storage_test.go
git commit -m "feat(data): add local storage abstraction for archived data"
```

---

## Task 4: GCS Storage Implementation

**Files:**
- Modify: `internal/data/storage.go`
- Test: `internal/data/storage_test.go`

**Step 1: Write the failing test**

```go
// Add to internal/data/storage_test.go

func TestGCSStorageParseURL(t *testing.T) {
	storage, err := NewGCSStorage("gs://ssmd-archive")
	if err != nil {
		t.Fatalf("NewGCSStorage failed: %v", err)
	}

	if storage.bucket != "ssmd-archive" {
		t.Errorf("expected bucket ssmd-archive, got %s", storage.bucket)
	}
	if storage.prefix != "" {
		t.Errorf("expected empty prefix, got %s", storage.prefix)
	}
}

func TestGCSStorageParseURLWithPrefix(t *testing.T) {
	storage, err := NewGCSStorage("gs://my-bucket/data/ssmd")
	if err != nil {
		t.Fatalf("NewGCSStorage failed: %v", err)
	}

	if storage.bucket != "my-bucket" {
		t.Errorf("expected bucket my-bucket, got %s", storage.bucket)
	}
	if storage.prefix != "data/ssmd" {
		t.Errorf("expected prefix data/ssmd, got %s", storage.prefix)
	}
}

func TestNewStorageLocal(t *testing.T) {
	storage, err := NewStorage("/tmp/data")
	if err != nil {
		t.Fatalf("NewStorage failed: %v", err)
	}

	_, ok := storage.(*LocalStorage)
	if !ok {
		t.Error("expected LocalStorage for local path")
	}
}

func TestNewStorageGCS(t *testing.T) {
	storage, err := NewStorage("gs://bucket/prefix")
	if err != nil {
		t.Fatalf("NewStorage failed: %v", err)
	}

	_, ok := storage.(*GCSStorage)
	if !ok {
		t.Error("expected GCSStorage for gs:// path")
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/data -run TestGCSStorage -v`
Expected: FAIL with "undefined: NewGCSStorage"

**Step 3: Write minimal implementation**

```go
// Add to internal/data/storage.go

import (
	"bytes"
	"os/exec"
	"strings"
)

// GCSStorage implements Storage for Google Cloud Storage via gsutil
type GCSStorage struct {
	bucket string
	prefix string
}

// NewGCSStorage creates a new GCS storage instance
func NewGCSStorage(gcsURL string) (*GCSStorage, error) {
	// Parse gs://bucket/prefix
	if !strings.HasPrefix(gcsURL, "gs://") {
		return nil, fmt.Errorf("invalid GCS URL: %s", gcsURL)
	}

	path := strings.TrimPrefix(gcsURL, "gs://")
	parts := strings.SplitN(path, "/", 2)

	bucket := parts[0]
	prefix := ""
	if len(parts) > 1 {
		prefix = parts[1]
	}

	return &GCSStorage{bucket: bucket, prefix: prefix}, nil
}

// NewStorage creates a Storage based on path type
func NewStorage(path string) (Storage, error) {
	if strings.HasPrefix(path, "gs://") {
		return NewGCSStorage(path)
	}
	return NewLocalStorage(path), nil
}

// gsutil runs a gsutil command and returns stdout
func (s *GCSStorage) gsutil(args ...string) ([]byte, error) {
	cmd := exec.Command("gsutil", args...)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("gsutil %v: %s", args, stderr.String())
	}
	return stdout.Bytes(), nil
}

// gcsPath builds a gs:// path
func (s *GCSStorage) gcsPath(parts ...string) string {
	allParts := []string{s.bucket}
	if s.prefix != "" {
		allParts = append(allParts, s.prefix)
	}
	allParts = append(allParts, parts...)
	return "gs://" + strings.Join(allParts, "/")
}

// ListFeeds returns all feed directories from GCS
func (s *GCSStorage) ListFeeds() ([]string, error) {
	output, err := s.gsutil("ls", s.gcsPath())
	if err != nil {
		return nil, err
	}

	var feeds []string
	for _, line := range strings.Split(string(output), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		// gs://bucket/prefix/feed/ -> feed
		line = strings.TrimSuffix(line, "/")
		parts := strings.Split(line, "/")
		feeds = append(feeds, parts[len(parts)-1])
	}
	sort.Strings(feeds)
	return feeds, nil
}

// ListDates returns all date directories for a feed from GCS
func (s *GCSStorage) ListDates(feed string) ([]string, error) {
	output, err := s.gsutil("ls", s.gcsPath(feed))
	if err != nil {
		return nil, err
	}

	var dates []string
	for _, line := range strings.Split(string(output), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		line = strings.TrimSuffix(line, "/")
		parts := strings.Split(line, "/")
		dates = append(dates, parts[len(parts)-1])
	}
	sort.Strings(dates)
	return dates, nil
}

// GetManifest reads and parses a manifest.json from GCS
func (s *GCSStorage) GetManifest(feed, date string) (*types.Manifest, error) {
	output, err := s.gsutil("cat", s.gcsPath(feed, date, "manifest.json"))
	if err != nil {
		return nil, err
	}

	var m types.Manifest
	if err := json.Unmarshal(output, &m); err != nil {
		return nil, fmt.Errorf("parsing manifest: %w", err)
	}
	return &m, nil
}

// ReadFile reads file contents from GCS
func (s *GCSStorage) ReadFile(feed, date, filename string) ([]byte, error) {
	return s.gsutil("cat", s.gcsPath(feed, date, filename))
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/data -run "TestGCSStorage|TestNewStorage" -v`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/data/storage.go internal/data/storage_test.go
git commit -m "feat(data): add GCS storage implementation via gsutil"
```

---

## Task 5: Implement `ssmd data list`

**Files:**
- Modify: `internal/cmd/data.go`
- Test: `internal/cmd/data_test.go`

**Step 1: Write the failing test**

```go
// Add to internal/cmd/data_test.go

func TestDataListOutput(t *testing.T) {
	// Create temp data directory
	tmp := t.TempDir()

	os.MkdirAll(filepath.Join(tmp, "kalshi", "2025-12-25"), 0755)
	manifest := `{"feed":"kalshi","date":"2025-12-25","format":"jsonl","files":[{"name":"1200.jsonl.gz","records":1500,"bytes":50000,"start":"2025-12-25T12:00:00Z","end":"2025-12-25T12:05:00Z","nats_start_seq":1,"nats_end_seq":1500}],"tickers":["INXD","KXBTC"],"message_types":["trade","ticker"],"has_gaps":false}`
	os.WriteFile(filepath.Join(tmp, "kalshi", "2025-12-25", "manifest.json"), []byte(manifest), 0644)

	// Set path flag
	dataPath = tmp
	dataOutputJSON = true
	dataFeed = ""
	dataFrom = ""
	dataTo = ""

	var buf bytes.Buffer
	dataListCmd.SetOut(&buf)

	err := runDataList(dataListCmd, []string{})
	if err != nil {
		t.Fatalf("runDataList failed: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "kalshi") {
		t.Error("expected output to contain kalshi")
	}
	if !strings.Contains(output, "2025-12-25") {
		t.Error("expected output to contain date")
	}
	if !strings.Contains(output, "1500") {
		t.Error("expected output to contain record count")
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/cmd -run TestDataListOutput -v`
Expected: FAIL (runDataList returns nil, no output)

**Step 3: Write minimal implementation**

```go
// Update internal/cmd/data.go - replace runDataList

import (
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/aaronwald/ssmd/internal/data"
	"github.com/aaronwald/ssmd/internal/types"
)

// DatasetInfo represents a dataset for list output
type DatasetInfo struct {
	Feed       string  `json:"feed"`
	Date       string  `json:"date"`
	Records    uint64  `json:"records"`
	Tickers    int     `json:"tickers"`
	SizeMB     float64 `json:"size_mb"`
	HasGaps    bool    `json:"has_gaps"`
}

func runDataList(cmd *cobra.Command, args []string) error {
	// Determine data path
	path := dataPath
	if path == "" {
		path = os.Getenv("SSMD_DATA_PATH")
	}
	if path == "" {
		return fmt.Errorf("data path not specified (use --path or SSMD_DATA_PATH)")
	}

	storage, err := data.NewStorage(path)
	if err != nil {
		return fmt.Errorf("creating storage: %w", err)
	}

	// List feeds
	feeds, err := storage.ListFeeds()
	if err != nil {
		return fmt.Errorf("listing feeds: %w", err)
	}

	// Filter by feed if specified
	if dataFeed != "" {
		filtered := []string{}
		for _, f := range feeds {
			if f == dataFeed {
				filtered = append(filtered, f)
			}
		}
		feeds = filtered
	}

	// Parse date range
	var fromDate, toDate time.Time
	if dataFrom != "" {
		fromDate, err = time.Parse("2006-01-02", dataFrom)
		if err != nil {
			return fmt.Errorf("invalid from date: %w", err)
		}
	}
	if dataTo != "" {
		toDate, err = time.Parse("2006-01-02", dataTo)
		if err != nil {
			return fmt.Errorf("invalid to date: %w", err)
		}
	}

	// Collect dataset info
	var datasets []DatasetInfo
	for _, feed := range feeds {
		dates, err := storage.ListDates(feed)
		if err != nil {
			continue // Skip feeds with errors
		}

		for _, date := range dates {
			// Filter by date range
			if dataFrom != "" || dataTo != "" {
				d, err := time.Parse("2006-01-02", date)
				if err != nil {
					continue
				}
				if dataFrom != "" && d.Before(fromDate) {
					continue
				}
				if dataTo != "" && d.After(toDate) {
					continue
				}
			}

			manifest, err := storage.GetManifest(feed, date)
			if err != nil {
				continue
			}

			datasets = append(datasets, DatasetInfo{
				Feed:    manifest.Feed,
				Date:    manifest.Date,
				Records: manifest.TotalRecords(),
				Tickers: len(manifest.Tickers),
				SizeMB:  float64(manifest.TotalBytes()) / 1024 / 1024,
				HasGaps: manifest.HasGaps,
			})
		}
	}

	// Output
	if dataOutputJSON {
		enc := json.NewEncoder(cmd.OutOrStdout())
		enc.SetIndent("", "  ")
		return enc.Encode(datasets)
	}

	// Table output
	fmt.Fprintf(cmd.OutOrStdout(), "%-12s %-12s %10s %8s %10s %s\n",
		"FEED", "DATE", "RECORDS", "TICKERS", "SIZE", "GAPS")
	for _, d := range datasets {
		gaps := ""
		if d.HasGaps {
			gaps = "YES"
		}
		fmt.Fprintf(cmd.OutOrStdout(), "%-12s %-12s %10d %8d %9.1fMB %s\n",
			d.Feed, d.Date, d.Records, d.Tickers, d.SizeMB, gaps)
	}

	return nil
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/cmd -run TestDataListOutput -v`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/cmd/data.go internal/cmd/data_test.go
git commit -m "feat(data): implement ssmd data list command"
```

---

## Task 6: Implement `ssmd data sample`

**Files:**
- Create: `internal/data/reader.go`
- Test: `internal/data/reader_test.go`
- Modify: `internal/cmd/data.go`

**Step 1: Write the failing test for reader**

```go
// internal/data/reader_test.go
package data

import (
	"compress/gzip"
	"os"
	"path/filepath"
	"testing"
)

func TestReadJSONLGZ(t *testing.T) {
	tmp := t.TempDir()

	// Create gzipped JSONL file
	filePath := filepath.Join(tmp, "test.jsonl.gz")
	f, _ := os.Create(filePath)
	gw := gzip.NewWriter(f)
	gw.Write([]byte(`{"type":"trade","ticker":"INXD","price":0.55}` + "\n"))
	gw.Write([]byte(`{"type":"trade","ticker":"KXBTC","price":0.42}` + "\n"))
	gw.Write([]byte(`{"type":"ticker","ticker":"INXD","yes_bid":0.50}` + "\n"))
	gw.Close()
	f.Close()

	records, err := ReadJSONLGZ(filePath, "", "", 10)
	if err != nil {
		t.Fatalf("ReadJSONLGZ failed: %v", err)
	}

	if len(records) != 3 {
		t.Errorf("expected 3 records, got %d", len(records))
	}
}

func TestReadJSONLGZWithFilter(t *testing.T) {
	tmp := t.TempDir()

	filePath := filepath.Join(tmp, "test.jsonl.gz")
	f, _ := os.Create(filePath)
	gw := gzip.NewWriter(f)
	gw.Write([]byte(`{"type":"trade","ticker":"INXD","price":0.55}` + "\n"))
	gw.Write([]byte(`{"type":"trade","ticker":"KXBTC","price":0.42}` + "\n"))
	gw.Write([]byte(`{"type":"ticker","ticker":"INXD","yes_bid":0.50}` + "\n"))
	gw.Close()
	f.Close()

	// Filter by ticker
	records, err := ReadJSONLGZ(filePath, "INXD", "", 10)
	if err != nil {
		t.Fatalf("ReadJSONLGZ failed: %v", err)
	}

	if len(records) != 2 {
		t.Errorf("expected 2 records for INXD, got %d", len(records))
	}

	// Filter by type
	records, err = ReadJSONLGZ(filePath, "", "trade", 10)
	if err != nil {
		t.Fatalf("ReadJSONLGZ failed: %v", err)
	}

	if len(records) != 2 {
		t.Errorf("expected 2 trade records, got %d", len(records))
	}
}

func TestReadJSONLGZLimit(t *testing.T) {
	tmp := t.TempDir()

	filePath := filepath.Join(tmp, "test.jsonl.gz")
	f, _ := os.Create(filePath)
	gw := gzip.NewWriter(f)
	for i := 0; i < 100; i++ {
		gw.Write([]byte(`{"type":"trade","ticker":"INXD"}` + "\n"))
	}
	gw.Close()
	f.Close()

	records, err := ReadJSONLGZ(filePath, "", "", 5)
	if err != nil {
		t.Fatalf("ReadJSONLGZ failed: %v", err)
	}

	if len(records) != 5 {
		t.Errorf("expected 5 records (limited), got %d", len(records))
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/data -run TestReadJSONLGZ -v`
Expected: FAIL with "undefined: ReadJSONLGZ"

**Step 3: Write minimal implementation**

```go
// internal/data/reader.go
package data

import (
	"bufio"
	"compress/gzip"
	"encoding/json"
	"os"
	"strings"
)

// ReadJSONLGZ reads records from a gzipped JSONL file with optional filters
func ReadJSONLGZ(path string, tickerFilter string, typeFilter string, limit int) ([]map[string]interface{}, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	gr, err := gzip.NewReader(f)
	if err != nil {
		return nil, err
	}
	defer gr.Close()

	var records []map[string]interface{}
	scanner := bufio.NewScanner(gr)

	for scanner.Scan() {
		if limit > 0 && len(records) >= limit {
			break
		}

		line := scanner.Text()
		if line == "" {
			continue
		}

		var record map[string]interface{}
		if err := json.Unmarshal([]byte(line), &record); err != nil {
			continue // Skip malformed lines
		}

		// Apply ticker filter
		if tickerFilter != "" {
			ticker, ok := record["ticker"].(string)
			if !ok {
				// Check nested msg.market_ticker
				if msg, ok := record["msg"].(map[string]interface{}); ok {
					ticker, _ = msg["market_ticker"].(string)
				}
			}
			if !strings.EqualFold(ticker, tickerFilter) {
				continue
			}
		}

		// Apply type filter
		if typeFilter != "" {
			msgType, _ := record["type"].(string)
			if !strings.EqualFold(msgType, typeFilter) {
				continue
			}
		}

		records = append(records, record)
	}

	return records, scanner.Err()
}

// ReadJSONLGZFromBytes reads records from gzipped JSONL bytes
func ReadJSONLGZFromBytes(data []byte, tickerFilter string, typeFilter string, limit int) ([]map[string]interface{}, error) {
	// Write to temp file and read (simplest approach for GCS)
	tmp, err := os.CreateTemp("", "ssmd-*.jsonl.gz")
	if err != nil {
		return nil, err
	}
	defer os.Remove(tmp.Name())
	defer tmp.Close()

	if _, err := tmp.Write(data); err != nil {
		return nil, err
	}
	tmp.Close()

	return ReadJSONLGZ(tmp.Name(), tickerFilter, typeFilter, limit)
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/data -run TestReadJSONLGZ -v`
Expected: PASS

**Step 5: Implement runDataSample**

```go
// Update internal/cmd/data.go - replace runDataSample

func runDataSample(cmd *cobra.Command, args []string) error {
	feed := args[0]
	date := args[1]

	path := dataPath
	if path == "" {
		path = os.Getenv("SSMD_DATA_PATH")
	}
	if path == "" {
		return fmt.Errorf("data path not specified (use --path or SSMD_DATA_PATH)")
	}

	storage, err := data.NewStorage(path)
	if err != nil {
		return fmt.Errorf("creating storage: %w", err)
	}

	// Get manifest to find files
	manifest, err := storage.GetManifest(feed, date)
	if err != nil {
		return fmt.Errorf("getting manifest: %w", err)
	}

	if len(manifest.Files) == 0 {
		return fmt.Errorf("no files in manifest for %s/%s", feed, date)
	}

	// Read from first file (or all files up to limit)
	var allRecords []map[string]interface{}
	remaining := dataLimit

	for _, file := range manifest.Files {
		if remaining <= 0 {
			break
		}

		fileData, err := storage.ReadFile(feed, date, file.Name)
		if err != nil {
			continue
		}

		records, err := data.ReadJSONLGZFromBytes(fileData, dataTicker, dataType, remaining)
		if err != nil {
			continue
		}

		allRecords = append(allRecords, records...)
		remaining -= len(records)
	}

	// Output
	if dataOutputJSON {
		enc := json.NewEncoder(cmd.OutOrStdout())
		enc.SetIndent("", "  ")
		return enc.Encode(allRecords)
	}

	// Pretty print each record
	for _, r := range allRecords {
		b, _ := json.MarshalIndent(r, "", "  ")
		fmt.Fprintln(cmd.OutOrStdout(), string(b))
	}

	return nil
}
```

**Step 6: Run all tests**

Run: `go test ./internal/... -v`
Expected: PASS

**Step 7: Commit**

```bash
git add internal/data/reader.go internal/data/reader_test.go internal/cmd/data.go
git commit -m "feat(data): implement ssmd data sample command"
```

---

## Task 7: Implement `ssmd data schema`

**Files:**
- Modify: `internal/cmd/data.go`
- Test: `internal/cmd/data_test.go`

**Step 1: Write the failing test**

```go
// Add to internal/cmd/data_test.go

func TestDataSchemaOutput(t *testing.T) {
	dataOutputJSON = true

	var buf bytes.Buffer
	dataSchemaCmd.SetOut(&buf)

	err := runDataSchema(dataSchemaCmd, []string{"kalshi", "orderbook"})
	if err != nil {
		t.Fatalf("runDataSchema failed: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "yes_bid") {
		t.Error("expected schema to contain yes_bid field")
	}
	if !strings.Contains(output, "spread") {
		t.Error("expected schema to contain spread derived field")
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/cmd -run TestDataSchemaOutput -v`
Expected: FAIL (runDataSchema returns nil)

**Step 3: Write minimal implementation**

```go
// Update internal/cmd/data.go - replace runDataSchema

// SchemaInfo represents a message type schema
type SchemaInfo struct {
	Type    string            `json:"type"`
	Fields  map[string]string `json:"fields"`
	Derived []string          `json:"derived,omitempty"`
}

// Known schemas for Kalshi
var knownSchemas = map[string]map[string]SchemaInfo{
	"kalshi": {
		"trade": {
			Type: "trade",
			Fields: map[string]string{
				"ticker":    "string",
				"price":     "number",
				"count":     "number",
				"side":      "string",
				"ts":        "number",
				"taker_side": "string",
			},
			Derived: []string{},
		},
		"ticker": {
			Type: "ticker",
			Fields: map[string]string{
				"ticker":         "string",
				"yes_bid":        "number",
				"yes_ask":        "number",
				"no_bid":         "number",
				"no_ask":         "number",
				"last_price":     "number",
				"volume":         "number",
				"open_interest":  "number",
				"ts":             "number",
			},
			Derived: []string{"spread", "midpoint"},
		},
		"orderbook": {
			Type: "orderbook",
			Fields: map[string]string{
				"ticker":   "string",
				"yes_bid":  "number",
				"yes_ask":  "number",
				"no_bid":   "number",
				"no_ask":   "number",
				"ts":       "number",
			},
			Derived: []string{"spread", "midpoint", "imbalance"},
		},
	},
}

func runDataSchema(cmd *cobra.Command, args []string) error {
	feed := args[0]
	msgType := args[1]

	feedSchemas, ok := knownSchemas[feed]
	if !ok {
		return fmt.Errorf("unknown feed: %s", feed)
	}

	schema, ok := feedSchemas[msgType]
	if !ok {
		return fmt.Errorf("unknown message type %s for feed %s", msgType, feed)
	}

	if dataOutputJSON {
		enc := json.NewEncoder(cmd.OutOrStdout())
		enc.SetIndent("", "  ")
		return enc.Encode(schema)
	}

	// Table output
	fmt.Fprintf(cmd.OutOrStdout(), "Schema: %s.%s\n\n", feed, msgType)
	fmt.Fprintf(cmd.OutOrStdout(), "Fields:\n")
	for name, typ := range schema.Fields {
		fmt.Fprintf(cmd.OutOrStdout(), "  %-20s %s\n", name, typ)
	}
	if len(schema.Derived) > 0 {
		fmt.Fprintf(cmd.OutOrStdout(), "\nDerived:\n")
		for _, d := range schema.Derived {
			fmt.Fprintf(cmd.OutOrStdout(), "  %s\n", d)
		}
	}

	return nil
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/cmd -run TestDataSchemaOutput -v`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/cmd/data.go internal/cmd/data_test.go
git commit -m "feat(data): implement ssmd data schema command"
```

---

## Task 8: Implement `ssmd data builders`

**Files:**
- Modify: `internal/cmd/data.go`
- Test: `internal/cmd/data_test.go`

**Step 1: Write the failing test**

```go
// Add to internal/cmd/data_test.go

func TestDataBuildersOutput(t *testing.T) {
	dataOutputJSON = true

	var buf bytes.Buffer
	dataBuildersCmd.SetOut(&buf)

	err := runDataBuilders(dataBuildersCmd, []string{})
	if err != nil {
		t.Fatalf("runDataBuilders failed: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "orderbook") {
		t.Error("expected builders to contain orderbook")
	}
	if !strings.Contains(output, "priceHistory") {
		t.Error("expected builders to contain priceHistory")
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/cmd -run TestDataBuildersOutput -v`
Expected: FAIL

**Step 3: Write minimal implementation**

```go
// Update internal/cmd/data.go - replace runDataBuilders

// BuilderInfo represents a state builder
type BuilderInfo struct {
	ID          string   `json:"id"`
	Description string   `json:"description"`
	Derived     []string `json:"derived"`
}

var stateBuilders = []BuilderInfo{
	{
		ID:          "orderbook",
		Description: "Maintains bid/ask levels from orderbook updates",
		Derived:     []string{"spread", "bestBid", "bestAsk", "bidDepth", "askDepth", "midpoint"},
	},
	{
		ID:          "priceHistory",
		Description: "Rolling window of price history",
		Derived:     []string{"last", "vwap", "returns", "high", "low", "volatility"},
	},
	{
		ID:          "volumeProfile",
		Description: "Buy/sell volume tracking",
		Derived:     []string{"buyVolume", "sellVolume", "totalVolume", "ratio", "average"},
	},
}

func runDataBuilders(cmd *cobra.Command, args []string) error {
	if dataOutputJSON {
		enc := json.NewEncoder(cmd.OutOrStdout())
		enc.SetIndent("", "  ")
		return enc.Encode(stateBuilders)
	}

	// Table output
	fmt.Fprintf(cmd.OutOrStdout(), "%-15s %-45s %s\n", "ID", "DESCRIPTION", "DERIVED FIELDS")
	for _, b := range stateBuilders {
		derived := strings.Join(b.Derived, ", ")
		fmt.Fprintf(cmd.OutOrStdout(), "%-15s %-45s %s\n", b.ID, b.Description, derived)
	}

	return nil
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/cmd -run TestDataBuildersOutput -v`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/cmd/data.go internal/cmd/data_test.go
git commit -m "feat(data): implement ssmd data builders command"
```

---

## Task 9: Fix --output flag handling

**Files:**
- Modify: `internal/cmd/data.go`

The current `--output` flag is a boolean, but should be a string to support `--output json`.

**Step 1: Update flag definition**

```go
// Update internal/cmd/data.go - change flag definitions

var (
	dataFeed      string
	dataFrom      string
	dataTo        string
	dataTicker    string
	dataLimit     int
	dataType      string
	dataOutput    string  // Changed from bool to string
	dataPath      string
)

func init() {
	// List flags
	dataListCmd.Flags().StringVar(&dataFeed, "feed", "", "Filter by feed name")
	dataListCmd.Flags().StringVar(&dataFrom, "from", "", "Start date (YYYY-MM-DD)")
	dataListCmd.Flags().StringVar(&dataTo, "to", "", "End date (YYYY-MM-DD)")
	dataListCmd.Flags().StringVar(&dataOutput, "output", "", "Output format (json)")
	dataListCmd.Flags().StringVar(&dataPath, "path", "", "Data path (default: $SSMD_DATA_PATH)")

	// Sample flags
	dataSampleCmd.Flags().StringVar(&dataTicker, "ticker", "", "Filter by ticker")
	dataSampleCmd.Flags().IntVar(&dataLimit, "limit", 10, "Max records to return")
	dataSampleCmd.Flags().StringVar(&dataType, "type", "", "Message type (trade, ticker, orderbook)")
	dataSampleCmd.Flags().StringVar(&dataOutput, "output", "", "Output format (json)")
	dataSampleCmd.Flags().StringVar(&dataPath, "path", "", "Data path")

	// Schema flags
	dataSchemaCmd.Flags().StringVar(&dataOutput, "output", "", "Output format (json)")

	// Builders flags
	dataBuildersCmd.Flags().StringVar(&dataOutput, "output", "", "Output format (json)")

	// ... rest unchanged
}

// Helper function
func isJSONOutput() bool {
	return dataOutput == "json"
}
```

**Step 2: Update all run functions to use isJSONOutput()**

Replace all `dataOutputJSON` with `isJSONOutput()` in the run functions.

**Step 3: Update tests**

```go
// Update tests to use string flag
dataOutput = "json"  // instead of dataOutputJSON = true
```

**Step 4: Run all tests**

Run: `go test ./internal/cmd -run TestData -v`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/cmd/data.go internal/cmd/data_test.go
git commit -m "fix(data): change --output flag to string for 'json' value"
```

---

## Task 10: Integration Test and Documentation

**Files:**
- Modify: `internal/cmd/data_test.go`
- Update: `TODO.md`

**Step 1: Add integration test**

```go
// Add to internal/cmd/data_test.go

func TestDataCommandsIntegration(t *testing.T) {
	// Create realistic test data structure
	tmp := t.TempDir()

	// Create kalshi/2025-12-25 with manifest and data file
	dataDir := filepath.Join(tmp, "kalshi", "2025-12-25")
	os.MkdirAll(dataDir, 0755)

	// Create manifest
	manifest := `{
		"feed": "kalshi",
		"date": "2025-12-25",
		"format": "jsonl",
		"rotation_interval": "5m",
		"files": [
			{"name": "1200.jsonl.gz", "records": 3, "bytes": 500, "start": "2025-12-25T12:00:00Z", "end": "2025-12-25T12:05:00Z", "nats_start_seq": 1, "nats_end_seq": 3}
		],
		"tickers": ["INXD-25001", "KXBTC-25001"],
		"message_types": ["trade", "ticker"],
		"has_gaps": false
	}`
	os.WriteFile(filepath.Join(dataDir, "manifest.json"), []byte(manifest), 0644)

	// Create gzipped data file
	dataFile, _ := os.Create(filepath.Join(dataDir, "1200.jsonl.gz"))
	gw := gzip.NewWriter(dataFile)
	gw.Write([]byte(`{"type":"trade","msg":{"market_ticker":"INXD-25001","price":55,"count":100}}` + "\n"))
	gw.Write([]byte(`{"type":"ticker","msg":{"market_ticker":"INXD-25001","yes_bid":50,"yes_ask":60}}` + "\n"))
	gw.Write([]byte(`{"type":"trade","msg":{"market_ticker":"KXBTC-25001","price":42,"count":50}}` + "\n"))
	gw.Close()
	dataFile.Close()

	// Test list
	dataPath = tmp
	dataOutput = "json"
	dataFeed = ""
	dataFrom = ""
	dataTo = ""

	var buf bytes.Buffer
	dataListCmd.SetOut(&buf)
	err := runDataList(dataListCmd, []string{})
	if err != nil {
		t.Fatalf("list failed: %v", err)
	}

	var datasets []DatasetInfo
	json.Unmarshal(buf.Bytes(), &datasets)
	if len(datasets) != 1 || datasets[0].Feed != "kalshi" {
		t.Errorf("unexpected list output: %v", datasets)
	}

	// Test sample
	buf.Reset()
	dataSampleCmd.SetOut(&buf)
	dataLimit = 10
	dataTicker = ""
	dataType = ""
	err = runDataSample(dataSampleCmd, []string{"kalshi", "2025-12-25"})
	if err != nil {
		t.Fatalf("sample failed: %v", err)
	}

	var records []map[string]interface{}
	json.Unmarshal(buf.Bytes(), &records)
	if len(records) != 3 {
		t.Errorf("expected 3 records, got %d", len(records))
	}

	// Test sample with ticker filter
	buf.Reset()
	dataTicker = "INXD-25001"
	err = runDataSample(dataSampleCmd, []string{"kalshi", "2025-12-25"})
	if err != nil {
		t.Fatalf("sample with filter failed: %v", err)
	}

	json.Unmarshal(buf.Bytes(), &records)
	if len(records) != 2 {
		t.Errorf("expected 2 INXD records, got %d", len(records))
	}
}
```

**Step 2: Run integration test**

Run: `go test ./internal/cmd -run TestDataCommandsIntegration -v`
Expected: PASS

**Step 3: Update TODO.md**

Mark the ssmd data CLI tasks as complete.

**Step 4: Run full test suite**

Run: `make test`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/cmd/data_test.go TODO.md
git commit -m "test(data): add integration tests for ssmd data commands"
```

---

## Summary

After completing all tasks, you will have:

1. **`ssmd data list`** - List datasets with filtering by feed and date range
2. **`ssmd data sample`** - Sample records with ticker and type filters
3. **`ssmd data schema`** - Show schema for message types
4. **`ssmd data builders`** - List available state builders

All commands support `--output json` for agent tool consumption and work with both local storage and GCS via gsutil.
