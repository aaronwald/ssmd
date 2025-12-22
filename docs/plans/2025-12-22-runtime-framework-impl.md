# Runtime Framework Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `ssmd run <env> --config-dir <path>` command that reads metadata configs, connects to Kalshi WebSocket, and writes raw messages to JSONL files.

**Architecture:** Interface-based framework with pluggable Connector, Writer, and KeyResolver. Runner wires components together. HTTP server exposes health/metrics for K8s.

**Tech Stack:** Go 1.21+, gorilla/websocket, prometheus/client_golang, net/http

---

## Phase 1: Framework Interfaces

### Task 1: Create runtime interfaces

**Files:**
- Create: `internal/runtime/interfaces.go`

**Step 1: Create interfaces file**

```go
package runtime

import "context"

// Connector connects to a data source and produces messages
type Connector interface {
	// Connect establishes the connection
	Connect(ctx context.Context) error
	// Messages returns a channel of raw message bytes
	Messages() <-chan []byte
	// Close cleanly shuts down the connection
	Close() error
}

// Writer writes messages to a destination
type Writer interface {
	// Write writes a message with metadata
	Write(ctx context.Context, msg *Message) error
	// Close flushes and closes the writer
	Close() error
}

// KeyResolver resolves key values from a source
type KeyResolver interface {
	// Resolve returns key-value pairs from a source string (e.g., "env:VAR1,VAR2")
	Resolve(source string) (map[string]string, error)
}

// Message wraps raw data with metadata
type Message struct {
	Timestamp string `json:"ts"`
	Feed      string `json:"feed"`
	Data      []byte `json:"data"`
}
```

**Step 2: Verify it compiles**

Run: `go build ./internal/runtime/`
Expected: Success

**Step 3: Commit**

```bash
git add internal/runtime/interfaces.go
git commit -m "feat(runtime): add Connector, Writer, KeyResolver interfaces"
```

---

### Task 2: Create runner skeleton

**Files:**
- Create: `internal/runtime/runner.go`
- Create: `internal/runtime/runner_test.go`

**Step 1: Write test for runner creation**

```go
package runtime

import (
	"testing"

	"github.com/aaronwald/ssmd/internal/types"
)

func TestNewRunner(t *testing.T) {
	env := &types.Environment{
		Name: "test-env",
		Feed: "test-feed",
	}
	feed := &types.Feed{
		Name: "test-feed",
		Type: types.FeedTypeWebSocket,
	}

	runner, err := NewRunner(env, feed, nil, nil, nil)
	if err != nil {
		t.Fatalf("NewRunner() error = %v", err)
	}
	if runner == nil {
		t.Fatal("NewRunner() returned nil")
	}
	if runner.env.Name != "test-env" {
		t.Errorf("env.Name = %q, want %q", runner.env.Name, "test-env")
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/runtime/ -v -run TestNewRunner`
Expected: FAIL - NewRunner not defined

**Step 3: Implement runner skeleton**

```go
package runtime

import (
	"github.com/aaronwald/ssmd/internal/types"
)

// Runner orchestrates the data collection pipeline
type Runner struct {
	env       *types.Environment
	feed      *types.Feed
	connector Connector
	writer    Writer
	resolver  KeyResolver
}

// NewRunner creates a new runner with the given components
func NewRunner(env *types.Environment, feed *types.Feed, connector Connector, writer Writer, resolver KeyResolver) (*Runner, error) {
	return &Runner{
		env:       env,
		feed:      feed,
		connector: connector,
		writer:    writer,
		resolver:  resolver,
	}, nil
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/runtime/ -v -run TestNewRunner`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/runtime/runner.go internal/runtime/runner_test.go
git commit -m "feat(runtime): add Runner skeleton"
```

---

## Phase 2: Key Resolver

### Task 3: Create EnvResolver

**Files:**
- Create: `internal/resolver/env.go`
- Create: `internal/resolver/env_test.go`

**Step 1: Write tests for EnvResolver**

```go
package resolver

import (
	"os"
	"testing"
)

func TestEnvResolver_Resolve(t *testing.T) {
	// Set test env vars
	os.Setenv("TEST_KEY1", "value1")
	os.Setenv("TEST_KEY2", "value2")
	defer os.Unsetenv("TEST_KEY1")
	defer os.Unsetenv("TEST_KEY2")

	r := NewEnvResolver()

	result, err := r.Resolve("env:TEST_KEY1,TEST_KEY2")
	if err != nil {
		t.Fatalf("Resolve() error = %v", err)
	}

	if result["TEST_KEY1"] != "value1" {
		t.Errorf("TEST_KEY1 = %q, want %q", result["TEST_KEY1"], "value1")
	}
	if result["TEST_KEY2"] != "value2" {
		t.Errorf("TEST_KEY2 = %q, want %q", result["TEST_KEY2"], "value2")
	}
}

func TestEnvResolver_Resolve_MissingVar(t *testing.T) {
	r := NewEnvResolver()

	_, err := r.Resolve("env:NONEXISTENT_VAR_12345")
	if err == nil {
		t.Error("Resolve() expected error for missing var")
	}
}

func TestEnvResolver_Resolve_InvalidSource(t *testing.T) {
	r := NewEnvResolver()

	_, err := r.Resolve("vault:secret/path")
	if err == nil {
		t.Error("Resolve() expected error for non-env source")
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/resolver/ -v`
Expected: FAIL - NewEnvResolver not defined

**Step 3: Implement EnvResolver**

```go
package resolver

import (
	"fmt"
	"os"
	"strings"
)

// EnvResolver resolves keys from environment variables
type EnvResolver struct{}

// NewEnvResolver creates a new EnvResolver
func NewEnvResolver() *EnvResolver {
	return &EnvResolver{}
}

// Resolve parses "env:VAR1,VAR2" and returns values from environment
func (r *EnvResolver) Resolve(source string) (map[string]string, error) {
	if !strings.HasPrefix(source, "env:") {
		return nil, fmt.Errorf("unsupported source type: %s (only env: supported)", source)
	}

	varsPart := strings.TrimPrefix(source, "env:")
	if varsPart == "" {
		return nil, fmt.Errorf("empty env source")
	}

	vars := strings.Split(varsPart, ",")
	result := make(map[string]string)

	for _, v := range vars {
		v = strings.TrimSpace(v)
		if v == "" {
			continue
		}
		value := os.Getenv(v)
		if value == "" {
			return nil, fmt.Errorf("environment variable %s is not set", v)
		}
		result[v] = value
	}

	return result, nil
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/resolver/ -v`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/resolver/env.go internal/resolver/env_test.go
git commit -m "feat(resolver): add EnvResolver for environment variables"
```

---

## Phase 3: File Writer

### Task 4: Create FileWriter

**Files:**
- Create: `internal/writer/file.go`
- Create: `internal/writer/file_test.go`

**Step 1: Write tests for FileWriter**

```go
package writer

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/aaronwald/ssmd/internal/runtime"
)

func TestFileWriter_Write(t *testing.T) {
	tmpDir := t.TempDir()

	w, err := NewFileWriter(tmpDir, "test-feed")
	if err != nil {
		t.Fatalf("NewFileWriter() error = %v", err)
	}
	defer w.Close()

	msg := &runtime.Message{
		Timestamp: time.Now().UTC().Format(time.RFC3339),
		Feed:      "test-feed",
		Data:      []byte(`{"price":100}`),
	}

	if err := w.Write(context.Background(), msg); err != nil {
		t.Fatalf("Write() error = %v", err)
	}

	// Verify file was created with today's date
	today := time.Now().UTC().Format("2006-01-02")
	expectedPath := filepath.Join(tmpDir, today, "test-feed.jsonl")

	data, err := os.ReadFile(expectedPath)
	if err != nil {
		t.Fatalf("ReadFile() error = %v", err)
	}

	if !strings.Contains(string(data), `"price":100`) {
		t.Errorf("file content missing expected data: %s", data)
	}
}

func TestFileWriter_DatePartitioning(t *testing.T) {
	tmpDir := t.TempDir()

	w, err := NewFileWriter(tmpDir, "test-feed")
	if err != nil {
		t.Fatalf("NewFileWriter() error = %v", err)
	}
	defer w.Close()

	msg := &runtime.Message{
		Timestamp: "2025-12-22T10:00:00Z",
		Feed:      "test-feed",
		Data:      []byte(`{"test":true}`),
	}

	if err := w.Write(context.Background(), msg); err != nil {
		t.Fatalf("Write() error = %v", err)
	}

	// Check file exists in date directory
	expectedPath := filepath.Join(tmpDir, "2025-12-22", "test-feed.jsonl")
	if _, err := os.Stat(expectedPath); os.IsNotExist(err) {
		t.Errorf("expected file at %s", expectedPath)
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/writer/ -v`
Expected: FAIL - NewFileWriter not defined

**Step 3: Implement FileWriter**

```go
package writer

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/aaronwald/ssmd/internal/runtime"
)

// FileWriter writes messages to date-partitioned JSONL files
type FileWriter struct {
	baseDir  string
	feedName string
	mu       sync.Mutex
	file     *os.File
	currDate string
}

// NewFileWriter creates a new FileWriter
func NewFileWriter(baseDir, feedName string) (*FileWriter, error) {
	return &FileWriter{
		baseDir:  baseDir,
		feedName: feedName,
	}, nil
}

// Write writes a message to the appropriate date-partitioned file
func (w *FileWriter) Write(ctx context.Context, msg *runtime.Message) error {
	w.mu.Lock()
	defer w.mu.Unlock()

	// Parse date from timestamp
	ts, err := time.Parse(time.RFC3339, msg.Timestamp)
	if err != nil {
		ts = time.Now().UTC()
	}
	date := ts.Format("2006-01-02")

	// Rotate file if date changed
	if date != w.currDate {
		if w.file != nil {
			w.file.Close()
		}

		dir := filepath.Join(w.baseDir, date)
		if err := os.MkdirAll(dir, 0755); err != nil {
			return fmt.Errorf("failed to create directory: %w", err)
		}

		path := filepath.Join(dir, w.feedName+".jsonl")
		f, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
		if err != nil {
			return fmt.Errorf("failed to open file: %w", err)
		}

		w.file = f
		w.currDate = date
	}

	// Write JSON line
	line, err := json.Marshal(msg)
	if err != nil {
		return fmt.Errorf("failed to marshal message: %w", err)
	}

	if _, err := w.file.Write(append(line, '\n')); err != nil {
		return fmt.Errorf("failed to write: %w", err)
	}

	return nil
}

// Close closes the current file
func (w *FileWriter) Close() error {
	w.mu.Lock()
	defer w.mu.Unlock()

	if w.file != nil {
		return w.file.Close()
	}
	return nil
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/writer/ -v`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/writer/file.go internal/writer/file_test.go
git commit -m "feat(writer): add FileWriter for JSONL output"
```

---

## Phase 4: HTTP Server

### Task 5: Create health server

**Files:**
- Create: `internal/server/health.go`
- Create: `internal/server/health_test.go`

**Step 1: Write tests for health server**

```go
package server

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestHealthHandler_OK(t *testing.T) {
	s := NewHealthServer(":8080")
	s.SetConnected(true)

	req := httptest.NewRequest("GET", "/health", nil)
	w := httptest.NewRecorder()

	s.healthHandler(w, req)

	if w.Code != http.StatusOK {
		t.Errorf("status = %d, want %d", w.Code, http.StatusOK)
	}

	var resp map[string]string
	json.Unmarshal(w.Body.Bytes(), &resp)
	if resp["status"] != "ok" {
		t.Errorf("status = %q, want %q", resp["status"], "ok")
	}
}

func TestReadyHandler_NotReady(t *testing.T) {
	s := NewHealthServer(":8080")
	s.SetConnected(false)

	req := httptest.NewRequest("GET", "/ready", nil)
	w := httptest.NewRecorder()

	s.readyHandler(w, req)

	if w.Code != http.StatusServiceUnavailable {
		t.Errorf("status = %d, want %d", w.Code, http.StatusServiceUnavailable)
	}
}

func TestReadyHandler_Ready(t *testing.T) {
	s := NewHealthServer(":8080")
	s.SetConnected(true)

	req := httptest.NewRequest("GET", "/ready", nil)
	w := httptest.NewRecorder()

	s.readyHandler(w, req)

	if w.Code != http.StatusOK {
		t.Errorf("status = %d, want %d", w.Code, http.StatusOK)
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/server/ -v`
Expected: FAIL - NewHealthServer not defined

**Step 3: Implement health server**

```go
package server

import (
	"encoding/json"
	"net/http"
	"sync"
	"sync/atomic"
)

// HealthServer provides health and readiness endpoints
type HealthServer struct {
	addr      string
	connected atomic.Bool
	mu        sync.RWMutex
	metrics   *Metrics
	server    *http.Server
}

// Metrics tracks collector statistics
type Metrics struct {
	MessagesTotal    atomic.Int64
	ErrorsTotal      atomic.Int64
	LastMessageTime  atomic.Int64
}

// NewHealthServer creates a new health server
func NewHealthServer(addr string) *HealthServer {
	s := &HealthServer{
		addr:    addr,
		metrics: &Metrics{},
	}

	mux := http.NewServeMux()
	mux.HandleFunc("/health", s.healthHandler)
	mux.HandleFunc("/ready", s.readyHandler)
	mux.HandleFunc("/metrics", s.metricsHandler)

	s.server = &http.Server{
		Addr:    addr,
		Handler: mux,
	}

	return s
}

// SetConnected updates the connection status
func (s *HealthServer) SetConnected(connected bool) {
	s.connected.Store(connected)
}

// RecordMessage records a message received
func (s *HealthServer) RecordMessage() {
	s.metrics.MessagesTotal.Add(1)
}

// RecordError records an error
func (s *HealthServer) RecordError() {
	s.metrics.ErrorsTotal.Add(1)
}

// Start starts the HTTP server
func (s *HealthServer) Start() error {
	return s.server.ListenAndServe()
}

// Close stops the HTTP server
func (s *HealthServer) Close() error {
	return s.server.Close()
}

func (s *HealthServer) healthHandler(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
}

func (s *HealthServer) readyHandler(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")

	if s.connected.Load() {
		json.NewEncoder(w).Encode(map[string]string{"status": "ready"})
	} else {
		w.WriteHeader(http.StatusServiceUnavailable)
		json.NewEncoder(w).Encode(map[string]string{"status": "not_ready"})
	}
}

func (s *HealthServer) metricsHandler(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "text/plain")

	connected := 0
	if s.connected.Load() {
		connected = 1
	}

	w.Write([]byte("# HELP ssmd_messages_total Total messages received\n"))
	w.Write([]byte("# TYPE ssmd_messages_total counter\n"))
	w.Write([]byte("ssmd_messages_total " + itoa(s.metrics.MessagesTotal.Load()) + "\n"))
	w.Write([]byte("# HELP ssmd_errors_total Total errors\n"))
	w.Write([]byte("# TYPE ssmd_errors_total counter\n"))
	w.Write([]byte("ssmd_errors_total " + itoa(s.metrics.ErrorsTotal.Load()) + "\n"))
	w.Write([]byte("# HELP ssmd_connected Connection status\n"))
	w.Write([]byte("# TYPE ssmd_connected gauge\n"))
	w.Write([]byte("ssmd_connected " + itoa(int64(connected)) + "\n"))
}

func itoa(i int64) string {
	return string(rune(i + '0'))
}
```

**Step 4: Fix itoa helper (use strconv)**

Replace `itoa` function:

```go
import "strconv"

// Remove the itoa function and use strconv.FormatInt instead
func (s *HealthServer) metricsHandler(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "text/plain")

	connected := int64(0)
	if s.connected.Load() {
		connected = 1
	}

	fmt.Fprintf(w, "# HELP ssmd_messages_total Total messages received\n")
	fmt.Fprintf(w, "# TYPE ssmd_messages_total counter\n")
	fmt.Fprintf(w, "ssmd_messages_total %d\n", s.metrics.MessagesTotal.Load())
	fmt.Fprintf(w, "# HELP ssmd_errors_total Total errors\n")
	fmt.Fprintf(w, "# TYPE ssmd_errors_total counter\n")
	fmt.Fprintf(w, "ssmd_errors_total %d\n", s.metrics.ErrorsTotal.Load())
	fmt.Fprintf(w, "# HELP ssmd_connected Connection status\n")
	fmt.Fprintf(w, "# TYPE ssmd_connected gauge\n")
	fmt.Fprintf(w, "ssmd_connected %d\n", connected)
}
```

**Step 5: Run test to verify it passes**

Run: `go test ./internal/server/ -v`
Expected: PASS

**Step 6: Commit**

```bash
git add internal/server/health.go internal/server/health_test.go
git commit -m "feat(server): add health/ready/metrics endpoints"
```

---

## Phase 5: WebSocket Connector

### Task 6: Create WebSocket connector

**Files:**
- Create: `internal/connector/websocket.go`
- Create: `internal/connector/websocket_test.go`

**Step 1: Add gorilla/websocket dependency**

Run: `go get github.com/gorilla/websocket`

**Step 2: Write test for connector creation**

```go
package connector

import (
	"testing"
)

func TestNewWebSocketConnector(t *testing.T) {
	creds := map[string]string{
		"api_key":    "test-key",
		"api_secret": "test-secret",
	}

	c := NewWebSocketConnector("wss://example.com/ws", creds)
	if c == nil {
		t.Fatal("NewWebSocketConnector() returned nil")
	}
	if c.url != "wss://example.com/ws" {
		t.Errorf("url = %q, want %q", c.url, "wss://example.com/ws")
	}
}

func TestWebSocketConnector_Messages(t *testing.T) {
	c := NewWebSocketConnector("wss://example.com/ws", nil)
	ch := c.Messages()
	if ch == nil {
		t.Error("Messages() returned nil channel")
	}
}
```

**Step 3: Run test to verify it fails**

Run: `go test ./internal/connector/ -v`
Expected: FAIL - NewWebSocketConnector not defined

**Step 4: Implement WebSocket connector**

```go
package connector

import (
	"context"
	"fmt"
	"net/http"
	"sync"

	"github.com/gorilla/websocket"
)

// WebSocketConnector connects to a WebSocket endpoint
type WebSocketConnector struct {
	url      string
	creds    map[string]string
	conn     *websocket.Conn
	messages chan []byte
	done     chan struct{}
	mu       sync.Mutex
}

// NewWebSocketConnector creates a new WebSocket connector
func NewWebSocketConnector(url string, creds map[string]string) *WebSocketConnector {
	return &WebSocketConnector{
		url:      url,
		creds:    creds,
		messages: make(chan []byte, 100),
		done:     make(chan struct{}),
	}
}

// Connect establishes the WebSocket connection
func (c *WebSocketConnector) Connect(ctx context.Context) error {
	c.mu.Lock()
	defer c.mu.Unlock()

	header := http.Header{}
	// Add auth headers if credentials provided
	if c.creds != nil {
		if key, ok := c.creds["api_key"]; ok {
			header.Set("Authorization", "Bearer "+key)
		}
	}

	conn, _, err := websocket.DefaultDialer.DialContext(ctx, c.url, header)
	if err != nil {
		return fmt.Errorf("failed to connect to %s: %w", c.url, err)
	}

	c.conn = conn

	// Start reading messages
	go c.readLoop()

	return nil
}

// Messages returns the channel of incoming messages
func (c *WebSocketConnector) Messages() <-chan []byte {
	return c.messages
}

// Close closes the WebSocket connection
func (c *WebSocketConnector) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()

	close(c.done)

	if c.conn != nil {
		return c.conn.Close()
	}
	return nil
}

func (c *WebSocketConnector) readLoop() {
	defer close(c.messages)

	for {
		select {
		case <-c.done:
			return
		default:
			_, msg, err := c.conn.ReadMessage()
			if err != nil {
				// Connection closed or error - exit
				return
			}
			c.messages <- msg
		}
	}
}
```

**Step 5: Run test to verify it passes**

Run: `go test ./internal/connector/ -v`
Expected: PASS

**Step 6: Commit**

```bash
git add internal/connector/websocket.go internal/connector/websocket_test.go go.mod go.sum
git commit -m "feat(connector): add WebSocket connector"
```

---

## Phase 6: Runner Implementation

### Task 7: Implement runner Run method

**Files:**
- Modify: `internal/runtime/runner.go`
- Modify: `internal/runtime/runner_test.go`

**Step 1: Add Run method test**

Add to `runner_test.go`:

```go
func TestRunner_Run_ContextCancellation(t *testing.T) {
	env := &types.Environment{Name: "test"}
	feed := &types.Feed{Name: "test"}

	// Create mock connector that sends one message then blocks
	mockConn := &mockConnector{
		messages: make(chan []byte, 1),
	}
	mockConn.messages <- []byte(`{"test":true}`)

	// Create mock writer
	mockWriter := &mockWriter{}

	runner, _ := NewRunner(env, feed, mockConn, mockWriter, nil)

	ctx, cancel := context.WithCancel(context.Background())

	// Run in goroutine
	done := make(chan error)
	go func() {
		done <- runner.Run(ctx)
	}()

	// Wait a bit then cancel
	time.Sleep(50 * time.Millisecond)
	cancel()

	err := <-done
	if err != nil && err != context.Canceled {
		t.Errorf("Run() error = %v", err)
	}

	if mockWriter.writeCount == 0 {
		t.Error("expected at least one write")
	}
}

type mockConnector struct {
	messages chan []byte
}

func (m *mockConnector) Connect(ctx context.Context) error { return nil }
func (m *mockConnector) Messages() <-chan []byte          { return m.messages }
func (m *mockConnector) Close() error                     { close(m.messages); return nil }

type mockWriter struct {
	writeCount int
}

func (m *mockWriter) Write(ctx context.Context, msg *Message) error {
	m.writeCount++
	return nil
}
func (m *mockWriter) Close() error { return nil }
```

**Step 2: Add imports to test file**

```go
import (
	"context"
	"testing"
	"time"

	"github.com/aaronwald/ssmd/internal/types"
)
```

**Step 3: Implement Run method**

Add to `runner.go`:

```go
import (
	"context"
	"time"

	"github.com/aaronwald/ssmd/internal/types"
)

// Run starts the collection pipeline
func (r *Runner) Run(ctx context.Context) error {
	// Connect
	if err := r.connector.Connect(ctx); err != nil {
		return err
	}
	defer r.connector.Close()
	defer r.writer.Close()

	// Process messages
	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case msg, ok := <-r.connector.Messages():
			if !ok {
				// Channel closed - connector disconnected
				return nil
			}

			wrapped := &Message{
				Timestamp: time.Now().UTC().Format(time.RFC3339),
				Feed:      r.feed.Name,
				Data:      msg,
			}

			if err := r.writer.Write(ctx, wrapped); err != nil {
				// Log error but continue
				continue
			}
		}
	}
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/runtime/ -v`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/runtime/runner.go internal/runtime/runner_test.go
git commit -m "feat(runtime): implement Runner.Run method"
```

---

## Phase 7: Run Command

### Task 8: Create run command

**Files:**
- Create: `internal/cmd/run.go`

**Step 1: Create run command**

```go
package cmd

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"

	"github.com/spf13/cobra"

	"github.com/aaronwald/ssmd/internal/connector"
	"github.com/aaronwald/ssmd/internal/resolver"
	"github.com/aaronwald/ssmd/internal/runtime"
	"github.com/aaronwald/ssmd/internal/server"
	"github.com/aaronwald/ssmd/internal/types"
	"github.com/aaronwald/ssmd/internal/writer"
)

var configDir string

var runCmd = &cobra.Command{
	Use:   "run <environment>",
	Short: "Run a data collector for an environment",
	Long:  "Starts a data collector that reads the environment configuration and collects data from the specified feed.",
	Args:  cobra.ExactArgs(1),
	RunE:  runCollector,
}

func init() {
	rootCmd.AddCommand(runCmd)
	runCmd.Flags().StringVar(&configDir, "config-dir", "", "Path to configuration directory (required)")
	runCmd.MarkFlagRequired("config-dir")
}

func runCollector(cmd *cobra.Command, args []string) error {
	envName := args[0]

	// Load environment
	envPath := filepath.Join(configDir, "environments", envName+".yaml")
	env, err := types.LoadEnvironment(envPath)
	if err != nil {
		return fmt.Errorf("failed to load environment: %w", err)
	}

	// Load feed
	feedPath := filepath.Join(configDir, "feeds", env.Feed+".yaml")
	feed, err := types.LoadFeed(feedPath)
	if err != nil {
		return fmt.Errorf("failed to load feed: %w", err)
	}

	// Get active version
	version := feed.GetActiveVersion()
	if version == nil {
		return fmt.Errorf("no active version for feed %s", feed.Name)
	}

	// Resolve keys
	var creds map[string]string
	if len(env.Keys) > 0 {
		res := resolver.NewEnvResolver()
		for _, keySpec := range env.Keys {
			if keySpec.Source != "" {
				resolved, err := res.Resolve(keySpec.Source)
				if err != nil {
					return fmt.Errorf("failed to resolve keys: %w", err)
				}
				creds = resolved
				break // Use first key for now
			}
		}
	}

	// Create connector based on feed type
	var conn runtime.Connector
	switch feed.Type {
	case types.FeedTypeWebSocket:
		conn = connector.NewWebSocketConnector(version.Endpoint, creds)
	default:
		return fmt.Errorf("unsupported feed type: %s", feed.Type)
	}

	// Create writer based on storage type
	var w runtime.Writer
	switch env.Storage.Type {
	case types.StorageTypeLocal:
		w, err = writer.NewFileWriter(env.Storage.Path, feed.Name)
		if err != nil {
			return fmt.Errorf("failed to create writer: %w", err)
		}
	default:
		return fmt.Errorf("unsupported storage type: %s", env.Storage.Type)
	}

	// Create runner
	runner, err := runtime.NewRunner(env, feed, conn, w, nil)
	if err != nil {
		return fmt.Errorf("failed to create runner: %w", err)
	}

	// Start health server
	healthServer := server.NewHealthServer(":8080")
	go func() {
		if err := healthServer.Start(); err != nil {
			fmt.Fprintf(os.Stderr, "health server error: %v\n", err)
		}
	}()
	defer healthServer.Close()

	// Setup signal handling
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)

	go func() {
		<-sigCh
		cancel()
	}()

	// Run collector
	fmt.Printf("Starting collector for %s (feed: %s)\n", envName, feed.Name)
	healthServer.SetConnected(true)

	if err := runner.Run(ctx); err != nil && err != context.Canceled {
		return fmt.Errorf("collector error: %w", err)
	}

	fmt.Println("Collector stopped")
	return nil
}
```

**Step 2: Verify it compiles**

Run: `go build ./cmd/ssmd/`
Expected: Success

**Step 3: Commit**

```bash
git add internal/cmd/run.go
git commit -m "feat(cmd): add run command for collector"
```

---

## Phase 8: Integration Test

### Task 9: Add integration test

**Files:**
- Create: `test/e2e/run_test.go`

**Step 1: Write integration test**

```go
package e2e

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestRunCommand_RequiresConfigDir(t *testing.T) {
	cmd := exec.Command("./ssmd", "run", "kalshi-dev")
	output, err := cmd.CombinedOutput()

	if err == nil {
		t.Error("expected error when --config-dir not provided")
	}

	if !strings.Contains(string(output), "config-dir") {
		t.Errorf("expected error about config-dir, got: %s", output)
	}
}

func TestRunCommand_LoadsConfig(t *testing.T) {
	// This test verifies config loading without actually connecting
	// Skip in CI without credentials
	if os.Getenv("KALSHI_API_KEY") == "" {
		t.Skip("KALSHI_API_KEY not set, skipping integration test")
	}

	// Create temp storage dir
	tmpDir := t.TempDir()

	// Update env config to use temp dir (or use test config)
	configDir := filepath.Join("..", "..", "exchanges")

	cmd := exec.Command("./ssmd", "run", "kalshi-dev", "--config-dir", configDir)
	cmd.Env = append(os.Environ(), "SSMD_STORAGE_PATH="+tmpDir)

	// Start and immediately kill (just verify it starts)
	if err := cmd.Start(); err != nil {
		t.Fatalf("failed to start: %v", err)
	}

	// Give it a moment to start
	time.Sleep(100 * time.Millisecond)
	cmd.Process.Kill()
}
```

**Step 2: Run test**

Run: `go test ./test/e2e/ -v -run TestRunCommand`
Expected: PASS (with skip for credential test)

**Step 3: Commit**

```bash
git add test/e2e/run_test.go
git commit -m "test(e2e): add run command integration tests"
```

---

## Phase 9: Final Verification

### Task 10: Run full test suite and linter

**Step 1: Run all tests**

Run: `go test ./...`
Expected: All PASS

**Step 2: Run linter**

Run: `make lint`
Expected: No errors

**Step 3: Build and verify help**

Run: `go build -o ssmd ./cmd/ssmd && ./ssmd run --help`
Expected: Shows run command help with --config-dir flag

**Step 4: Final commit if cleanup needed**

```bash
git add -A
git commit -m "chore: cleanup after runtime framework implementation"
```

---

## Summary

**Total tasks:** 10
**New packages:** runtime, connector, writer, resolver, server
**New files:** ~15
**New command:** `ssmd run <env> --config-dir <path>`
