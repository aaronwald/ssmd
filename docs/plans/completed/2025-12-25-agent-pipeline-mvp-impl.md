# Agent Pipeline MVP Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build an interactive REPL for signal development with ssmd-data HTTP API and LangGraph agent.

**Architecture:** Go HTTP service (`ssmd-data`) serves archived market data. Deno REPL (`ssmd-agent`) uses LangGraph.js with Claude to generate, validate, and deploy TypeScript signals via tools.

**Tech Stack:** Go 1.21+ (ssmd-data), Deno 2.x (ssmd-agent), LangGraph.js, Anthropic SDK, Zod

---

## Task 1: ssmd-data HTTP Server Skeleton

**Files:**
- Create: `cmd/ssmd-data/main.go`
- Create: `internal/api/server.go`
- Create: `internal/api/middleware.go`

**Step 1: Create server skeleton**

```go
// internal/api/server.go
package api

import (
	"encoding/json"
	"log"
	"net/http"
	"os"

	"github.com/aaronwald/ssmd/internal/data"
)

type Server struct {
	storage data.Storage
	apiKey  string
	mux     *http.ServeMux
}

func NewServer(storage data.Storage, apiKey string) *Server {
	s := &Server{
		storage: storage,
		apiKey:  apiKey,
		mux:     http.NewServeMux(),
	}
	s.routes()
	return s
}

func (s *Server) routes() {
	s.mux.HandleFunc("GET /health", s.handleHealth)
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	s.mux.ServeHTTP(w, r)
}

func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
}
```

**Step 2: Create API key middleware**

```go
// internal/api/middleware.go
package api

import "net/http"

func (s *Server) requireAPIKey(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		key := r.Header.Get("X-API-Key")
		if key == "" || key != s.apiKey {
			http.Error(w, `{"error":"unauthorized"}`, http.StatusUnauthorized)
			return
		}
		next(w, r)
	}
}
```

**Step 3: Create main entry point**

```go
// cmd/ssmd-data/main.go
package main

import (
	"log"
	"net/http"
	"os"

	"github.com/aaronwald/ssmd/internal/api"
	"github.com/aaronwald/ssmd/internal/data"
)

func main() {
	dataPath := os.Getenv("SSMD_DATA_PATH")
	if dataPath == "" {
		log.Fatal("SSMD_DATA_PATH required")
	}

	apiKey := os.Getenv("SSMD_API_KEY")
	if apiKey == "" {
		log.Fatal("SSMD_API_KEY required")
	}

	port := os.Getenv("PORT")
	if port == "" {
		port = "8080"
	}

	storage, err := data.NewStorage(dataPath)
	if err != nil {
		log.Fatalf("creating storage: %v", err)
	}

	server := api.NewServer(storage, apiKey)

	log.Printf("ssmd-data listening on :%s", port)
	if err := http.ListenAndServe(":"+port, server); err != nil {
		log.Fatal(err)
	}
}
```

**Step 4: Verify it builds**

Run: `go build ./cmd/ssmd-data`
Expected: Binary created, no errors

**Step 5: Test health endpoint manually**

Run: `SSMD_DATA_PATH=/tmp SSMD_API_KEY=test go run ./cmd/ssmd-data &`
Run: `curl http://localhost:8080/health`
Expected: `{"status":"ok"}`
Run: `pkill -f ssmd-data`

**Step 6: Commit**

```bash
git add cmd/ssmd-data/ internal/api/
git commit -m "feat(api): add ssmd-data server skeleton with health endpoint"
```

---

## Task 2: ssmd-data /datasets Endpoint

**Files:**
- Modify: `internal/api/server.go`
- Create: `internal/api/handlers.go`
- Create: `internal/api/server_test.go`

**Step 1: Write the failing test**

```go
// internal/api/server_test.go
package api

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/aaronwald/ssmd/internal/types"
)

type mockStorage struct {
	feeds     []string
	dates     map[string][]string
	manifests map[string]map[string]*types.Manifest
}

func (m *mockStorage) ListFeeds() ([]string, error) {
	return m.feeds, nil
}

func (m *mockStorage) ListDates(feed string) ([]string, error) {
	return m.dates[feed], nil
}

func (m *mockStorage) GetManifest(feed, date string) (*types.Manifest, error) {
	return m.manifests[feed][date], nil
}

func (m *mockStorage) ReadFile(feed, date, filename string) ([]byte, error) {
	return nil, nil
}

func TestDatasetsEndpoint(t *testing.T) {
	storage := &mockStorage{
		feeds: []string{"kalshi"},
		dates: map[string][]string{"kalshi": {"2025-12-25"}},
		manifests: map[string]map[string]*types.Manifest{
			"kalshi": {
				"2025-12-25": {
					Feed:    "kalshi",
					Date:    "2025-12-25",
					Tickers: []string{"INXD-25001"},
					Files:   []types.FileEntry{{Records: 1000, Bytes: 50000}},
				},
			},
		},
	}

	server := NewServer(storage, "test-key")

	req := httptest.NewRequest("GET", "/datasets", nil)
	req.Header.Set("X-API-Key", "test-key")
	rec := httptest.NewRecorder()

	server.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Errorf("expected 200, got %d", rec.Code)
	}

	var datasets []DatasetInfo
	if err := json.NewDecoder(rec.Body).Decode(&datasets); err != nil {
		t.Fatalf("decoding response: %v", err)
	}

	if len(datasets) != 1 {
		t.Fatalf("expected 1 dataset, got %d", len(datasets))
	}

	if datasets[0].Feed != "kalshi" {
		t.Errorf("expected feed kalshi, got %s", datasets[0].Feed)
	}
}

func TestDatasetsRequiresAPIKey(t *testing.T) {
	server := NewServer(&mockStorage{}, "secret")

	req := httptest.NewRequest("GET", "/datasets", nil)
	rec := httptest.NewRecorder()

	server.ServeHTTP(rec, req)

	if rec.Code != http.StatusUnauthorized {
		t.Errorf("expected 401, got %d", rec.Code)
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/api/... -v -run TestDatasets`
Expected: FAIL (DatasetInfo not defined, handler not registered)

**Step 3: Create handlers with DatasetInfo type**

```go
// internal/api/handlers.go
package api

import (
	"encoding/json"
	"net/http"
	"time"
)

// DatasetInfo represents a dataset in API responses
type DatasetInfo struct {
	Feed    string  `json:"feed"`
	Date    string  `json:"date"`
	Records uint64  `json:"records"`
	Tickers int     `json:"tickers"`
	SizeMB  float64 `json:"size_mb"`
	HasGaps bool    `json:"has_gaps"`
}

func (s *Server) handleDatasets(w http.ResponseWriter, r *http.Request) {
	// Parse query params
	feedFilter := r.URL.Query().Get("feed")
	fromStr := r.URL.Query().Get("from")
	toStr := r.URL.Query().Get("to")

	var fromDate, toDate time.Time
	var err error
	if fromStr != "" {
		fromDate, err = time.Parse("2006-01-02", fromStr)
		if err != nil {
			http.Error(w, `{"error":"invalid from date"}`, http.StatusBadRequest)
			return
		}
	}
	if toStr != "" {
		toDate, err = time.Parse("2006-01-02", toStr)
		if err != nil {
			http.Error(w, `{"error":"invalid to date"}`, http.StatusBadRequest)
			return
		}
	}

	feeds, err := s.storage.ListFeeds()
	if err != nil {
		http.Error(w, `{"error":"listing feeds"}`, http.StatusInternalServerError)
		return
	}

	// Filter feeds
	if feedFilter != "" {
		filtered := []string{}
		for _, f := range feeds {
			if f == feedFilter {
				filtered = append(filtered, f)
			}
		}
		feeds = filtered
	}

	var datasets []DatasetInfo
	for _, feed := range feeds {
		dates, err := s.storage.ListDates(feed)
		if err != nil {
			continue
		}

		for _, date := range dates {
			// Date range filter
			if fromStr != "" || toStr != "" {
				d, err := time.Parse("2006-01-02", date)
				if err != nil {
					continue
				}
				if fromStr != "" && d.Before(fromDate) {
					continue
				}
				if toStr != "" && d.After(toDate) {
					continue
				}
			}

			manifest, err := s.storage.GetManifest(feed, date)
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

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(datasets)
}
```

**Step 4: Register the route in server.go**

Add to `routes()` in `internal/api/server.go`:

```go
func (s *Server) routes() {
	s.mux.HandleFunc("GET /health", s.handleHealth)
	s.mux.HandleFunc("GET /datasets", s.requireAPIKey(s.handleDatasets))
}
```

**Step 5: Run test to verify it passes**

Run: `go test ./internal/api/... -v -run TestDatasets`
Expected: PASS

**Step 6: Commit**

```bash
git add internal/api/
git commit -m "feat(api): add /datasets endpoint with filtering"
```

---

## Task 3: ssmd-data /datasets/{feed}/{date}/sample Endpoint

**Files:**
- Modify: `internal/api/handlers.go`
- Modify: `internal/api/server.go`
- Modify: `internal/api/server_test.go`

**Step 1: Write the failing test**

Add to `internal/api/server_test.go`:

```go
func TestSampleEndpoint(t *testing.T) {
	// Create mock with file data
	storage := &mockStorage{
		manifests: map[string]map[string]*types.Manifest{
			"kalshi": {
				"2025-12-25": {
					Feed: "kalshi",
					Date: "2025-12-25",
					Files: []types.FileEntry{
						{Name: "data.jsonl.gz"},
					},
				},
			},
		},
		fileData: map[string][]byte{
			"kalshi/2025-12-25/data.jsonl.gz": createGzipJSONL([]map[string]interface{}{
				{"type": "orderbook", "ticker": "INXD", "yes_bid": 0.45},
				{"type": "orderbook", "ticker": "INXD", "yes_ask": 0.55},
			}),
		},
	}

	server := NewServer(storage, "test-key")

	req := httptest.NewRequest("GET", "/datasets/kalshi/2025-12-25/sample?limit=2", nil)
	req.Header.Set("X-API-Key", "test-key")
	rec := httptest.NewRecorder()

	server.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Errorf("expected 200, got %d: %s", rec.Code, rec.Body.String())
	}

	var records []map[string]interface{}
	if err := json.NewDecoder(rec.Body).Decode(&records); err != nil {
		t.Fatalf("decoding response: %v", err)
	}

	if len(records) != 2 {
		t.Errorf("expected 2 records, got %d", len(records))
	}
}

// Add fileData to mockStorage
type mockStorage struct {
	feeds     []string
	dates     map[string][]string
	manifests map[string]map[string]*types.Manifest
	fileData  map[string][]byte
}

func (m *mockStorage) ReadFile(feed, date, filename string) ([]byte, error) {
	key := feed + "/" + date + "/" + filename
	if data, ok := m.fileData[key]; ok {
		return data, nil
	}
	return nil, fmt.Errorf("file not found")
}

// Helper to create gzipped JSONL
func createGzipJSONL(records []map[string]interface{}) []byte {
	var buf bytes.Buffer
	gw := gzip.NewWriter(&buf)
	for _, r := range records {
		b, _ := json.Marshal(r)
		gw.Write(b)
		gw.Write([]byte("\n"))
	}
	gw.Close()
	return buf.Bytes()
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/api/... -v -run TestSample`
Expected: FAIL (handler not registered, 404)

**Step 3: Add sample handler**

Add to `internal/api/handlers.go`:

```go
func (s *Server) handleSample(w http.ResponseWriter, r *http.Request) {
	feed := r.PathValue("feed")
	date := r.PathValue("date")

	tickerFilter := r.URL.Query().Get("ticker")
	typeFilter := r.URL.Query().Get("type")
	limitStr := r.URL.Query().Get("limit")

	limit := 10
	if limitStr != "" {
		if l, err := strconv.Atoi(limitStr); err == nil && l > 0 {
			limit = l
		}
	}

	manifest, err := s.storage.GetManifest(feed, date)
	if err != nil {
		http.Error(w, `{"error":"dataset not found"}`, http.StatusNotFound)
		return
	}

	var allRecords []map[string]interface{}
	remaining := limit

	for _, file := range manifest.Files {
		if remaining <= 0 {
			break
		}

		fileData, err := s.storage.ReadFile(feed, date, file.Name)
		if err != nil {
			continue
		}

		records, err := data.ReadJSONLGZFromBytes(fileData, tickerFilter, typeFilter, remaining)
		if err != nil {
			continue
		}

		allRecords = append(allRecords, records...)
		remaining -= len(records)
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(allRecords)
}
```

Add import for `strconv` and register route in `server.go`:

```go
s.mux.HandleFunc("GET /datasets/{feed}/{date}/sample", s.requireAPIKey(s.handleSample))
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/api/... -v -run TestSample`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/api/
git commit -m "feat(api): add /datasets/{feed}/{date}/sample endpoint"
```

---

## Task 4: ssmd-data /schema and /builders Endpoints

**Files:**
- Modify: `internal/api/handlers.go`
- Modify: `internal/api/server.go`
- Modify: `internal/api/server_test.go`

**Step 1: Write failing tests**

Add to `internal/api/server_test.go`:

```go
func TestSchemaEndpoint(t *testing.T) {
	server := NewServer(&mockStorage{}, "test-key")

	req := httptest.NewRequest("GET", "/schema/kalshi/orderbook", nil)
	req.Header.Set("X-API-Key", "test-key")
	rec := httptest.NewRecorder()

	server.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Errorf("expected 200, got %d", rec.Code)
	}

	var schema SchemaInfo
	if err := json.NewDecoder(rec.Body).Decode(&schema); err != nil {
		t.Fatalf("decoding: %v", err)
	}

	if schema.Type != "orderbook" {
		t.Errorf("expected type orderbook, got %s", schema.Type)
	}
}

func TestBuildersEndpoint(t *testing.T) {
	server := NewServer(&mockStorage{}, "test-key")

	req := httptest.NewRequest("GET", "/builders", nil)
	req.Header.Set("X-API-Key", "test-key")
	rec := httptest.NewRecorder()

	server.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Errorf("expected 200, got %d", rec.Code)
	}

	var builders []BuilderInfo
	if err := json.NewDecoder(rec.Body).Decode(&builders); err != nil {
		t.Fatalf("decoding: %v", err)
	}

	if len(builders) == 0 {
		t.Error("expected at least one builder")
	}
}
```

**Step 2: Run tests to verify they fail**

Run: `go test ./internal/api/... -v -run "TestSchema|TestBuilders"`
Expected: FAIL

**Step 3: Add schema and builder types and handlers**

Add to `internal/api/handlers.go`:

```go
// SchemaInfo represents a message type schema
type SchemaInfo struct {
	Type    string            `json:"type"`
	Fields  map[string]string `json:"fields"`
	Derived []string          `json:"derived,omitempty"`
}

// BuilderInfo represents a state builder
type BuilderInfo struct {
	ID          string   `json:"id"`
	Description string   `json:"description"`
	Derived     []string `json:"derived"`
}

// Known schemas (same as cmd/data.go)
var knownSchemas = map[string]map[string]SchemaInfo{
	"kalshi": {
		"trade": {
			Type: "trade",
			Fields: map[string]string{
				"ticker": "string", "price": "number", "count": "number",
				"side": "string", "ts": "number", "taker_side": "string",
			},
		},
		"ticker": {
			Type: "ticker",
			Fields: map[string]string{
				"ticker": "string", "yes_bid": "number", "yes_ask": "number",
				"no_bid": "number", "no_ask": "number", "last_price": "number",
				"volume": "number", "open_interest": "number", "ts": "number",
			},
			Derived: []string{"spread", "midpoint"},
		},
		"orderbook": {
			Type: "orderbook",
			Fields: map[string]string{
				"ticker": "string", "yes_bid": "number", "yes_ask": "number",
				"no_bid": "number", "no_ask": "number", "ts": "number",
			},
			Derived: []string{"spread", "midpoint", "imbalance"},
		},
	},
}

var stateBuilders = []BuilderInfo{
	{ID: "orderbook", Description: "Maintains bid/ask levels from orderbook updates",
		Derived: []string{"spread", "bestBid", "bestAsk", "bidDepth", "askDepth", "midpoint"}},
	{ID: "priceHistory", Description: "Rolling window of price history",
		Derived: []string{"last", "vwap", "returns", "high", "low", "volatility"}},
	{ID: "volumeProfile", Description: "Buy/sell volume tracking",
		Derived: []string{"buyVolume", "sellVolume", "totalVolume", "ratio", "average"}},
}

func (s *Server) handleSchema(w http.ResponseWriter, r *http.Request) {
	feed := r.PathValue("feed")
	msgType := r.PathValue("type")

	feedSchemas, ok := knownSchemas[feed]
	if !ok {
		http.Error(w, `{"error":"unknown feed"}`, http.StatusNotFound)
		return
	}

	schema, ok := feedSchemas[msgType]
	if !ok {
		http.Error(w, `{"error":"unknown message type"}`, http.StatusNotFound)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(schema)
}

func (s *Server) handleBuilders(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(stateBuilders)
}
```

Register routes in `server.go`:

```go
s.mux.HandleFunc("GET /schema/{feed}/{type}", s.requireAPIKey(s.handleSchema))
s.mux.HandleFunc("GET /builders", s.requireAPIKey(s.handleBuilders))
```

**Step 4: Run tests to verify they pass**

Run: `go test ./internal/api/... -v -run "TestSchema|TestBuilders"`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/api/
git commit -m "feat(api): add /schema and /builders endpoints"
```

---

## Task 5: ssmd-data Dockerfile

**Files:**
- Create: `cmd/ssmd-data/Dockerfile`

**Step 1: Create Dockerfile**

```dockerfile
# cmd/ssmd-data/Dockerfile
FROM golang:1.21-alpine AS builder

WORKDIR /app

COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 go build -o ssmd-data ./cmd/ssmd-data

FROM alpine:3.19

RUN adduser -D -s /bin/false ssmd
COPY --from=builder /app/ssmd-data /usr/local/bin/

USER ssmd
EXPOSE 8080

CMD ["ssmd-data"]
```

**Step 2: Test Docker build**

Run: `docker build -f cmd/ssmd-data/Dockerfile -t ssmd-data:dev .`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add cmd/ssmd-data/Dockerfile
git commit -m "build: add Dockerfile for ssmd-data"
```

---

## Task 6: Agent REPL - Dependencies and Config

**Files:**
- Modify: `ssmd-agent/deno.json`
- Create: `ssmd-agent/src/config.ts`

**Step 1: Update deno.json with dependencies**

```json
{
  "tasks": {
    "start": "deno run --allow-net --allow-env src/main.ts",
    "agent": "deno run --allow-net --allow-env --allow-read src/cli.ts",
    "dev": "deno run --watch --allow-net --allow-env src/main.ts",
    "check": "deno check src/main.ts src/cli.ts"
  },
  "imports": {
    "@langchain/anthropic": "npm:@langchain/anthropic@^0.3",
    "@langchain/langgraph": "npm:@langchain/langgraph@^0.2",
    "@langchain/core": "npm:@langchain/core@^0.3",
    "zod": "npm:zod@^3.23",
    "yaml": "npm:yaml@^2.3"
  },
  "compilerOptions": {
    "strict": true
  }
}
```

**Step 2: Create config module**

```typescript
// ssmd-agent/src/config.ts
export const config = {
  dataUrl: Deno.env.get("SSMD_DATA_URL") ?? "http://localhost:8080",
  dataApiKey: Deno.env.get("SSMD_DATA_API_KEY") ?? "",
  anthropicApiKey: Deno.env.get("ANTHROPIC_API_KEY") ?? "",
  model: Deno.env.get("SSMD_MODEL") ?? "claude-sonnet-4-20250514",
  skillsPath: Deno.env.get("SSMD_SKILLS_PATH") ?? "./skills",
  signalsPath: Deno.env.get("SSMD_SIGNALS_PATH") ?? "./signals",
};

export function validateConfig(): void {
  if (!config.dataApiKey) {
    throw new Error("SSMD_DATA_API_KEY required");
  }
  if (!config.anthropicApiKey) {
    throw new Error("ANTHROPIC_API_KEY required");
  }
}
```

**Step 3: Verify deno check passes**

Run: `cd ssmd-agent && deno check src/config.ts`
Expected: No errors

**Step 4: Commit**

```bash
git add ssmd-agent/
git commit -m "feat(agent): add LangGraph dependencies and config"
```

---

## Task 7: Agent Skills Loader

**Files:**
- Create: `ssmd-agent/src/agent/skills.ts`
- Create: `ssmd-agent/skills/explore-data.md`

**Step 1: Create skills loader**

```typescript
// ssmd-agent/src/agent/skills.ts
import { config } from "../config.ts";

export interface Skill {
  name: string;
  description: string;
  content: string;
}

interface Frontmatter {
  name: string;
  description: string;
}

function parseFrontmatter(text: string): { frontmatter: Frontmatter; body: string } {
  const match = text.match(/^---\n([\s\S]*?)\n---\n([\s\S]*)$/);
  if (!match) {
    return {
      frontmatter: { name: "unknown", description: "" },
      body: text,
    };
  }

  const yamlStr = match[1];
  const body = match[2];

  // Simple YAML parsing for name/description
  const lines = yamlStr.split("\n");
  const frontmatter: Frontmatter = { name: "", description: "" };

  for (const line of lines) {
    const [key, ...rest] = line.split(":");
    const value = rest.join(":").trim();
    if (key.trim() === "name") frontmatter.name = value;
    if (key.trim() === "description") frontmatter.description = value;
  }

  return { frontmatter, body };
}

export async function loadSkills(): Promise<Skill[]> {
  const skills: Skill[] = [];

  try {
    for await (const entry of Deno.readDir(config.skillsPath)) {
      if (entry.isFile && entry.name.endsWith(".md")) {
        const content = await Deno.readTextFile(`${config.skillsPath}/${entry.name}`);
        const { frontmatter, body } = parseFrontmatter(content);
        skills.push({
          name: frontmatter.name,
          description: frontmatter.description,
          content: body,
        });
      }
    }
  } catch (e) {
    if (!(e instanceof Deno.errors.NotFound)) {
      throw e;
    }
    // Skills directory doesn't exist yet, return empty
  }

  return skills;
}
```

**Step 2: Create first skill file**

```markdown
---
name: explore-data
description: How to discover and understand available market data
---

# Exploring Data

When you need to understand what data is available:

1. Use `list_datasets` to see available feeds and dates
2. Use `sample_data` to look at actual records
3. Use `get_schema` to understand field types
4. Use `list_builders` to see what state can be derived

## Key Patterns

- Kalshi uses prediction market format: yes_bid, yes_ask
- Spread = yes_ask - yes_bid
- All timestamps are Unix milliseconds (UTC)

## Watch Out For

- Gaps in data (check has_gaps in dataset info)
- Low volume tickers (noisy)
- Market hours (Kalshi has weekend gaps)
```

**Step 3: Verify deno check passes**

Run: `cd ssmd-agent && deno check src/agent/skills.ts`
Expected: No errors

**Step 4: Commit**

```bash
git add ssmd-agent/
git commit -m "feat(agent): add skills loader and explore-data skill"
```

---

## Task 8: Agent Data Tools

**Files:**
- Create: `ssmd-agent/src/agent/tools.ts`

**Step 1: Create tool definitions**

```typescript
// ssmd-agent/src/agent/tools.ts
import { tool } from "@langchain/core/tools";
import { z } from "zod";
import { config } from "../config.ts";

async function apiRequest<T>(path: string): Promise<T> {
  const res = await fetch(`${config.dataUrl}${path}`, {
    headers: { "X-API-Key": config.dataApiKey },
  });
  if (!res.ok) {
    throw new Error(`API error: ${res.status} ${await res.text()}`);
  }
  return res.json();
}

export const listDatasets = tool(
  async ({ feed, from, to }) => {
    const params = new URLSearchParams();
    if (feed) params.set("feed", feed);
    if (from) params.set("from", from);
    if (to) params.set("to", to);

    const path = `/datasets${params.toString() ? "?" + params : ""}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "list_datasets",
    description: "List available market data datasets. Returns feed, date, record count, ticker count.",
    schema: z.object({
      feed: z.string().optional().describe("Filter by feed name (e.g., 'kalshi')"),
      from: z.string().optional().describe("Start date YYYY-MM-DD"),
      to: z.string().optional().describe("End date YYYY-MM-DD"),
    }),
  }
);

export const sampleData = tool(
  async ({ feed, date, ticker, type, limit }) => {
    const params = new URLSearchParams();
    if (ticker) params.set("ticker", ticker);
    if (type) params.set("type", type);
    if (limit) params.set("limit", String(limit));

    const path = `/datasets/${feed}/${date}/sample${params.toString() ? "?" + params : ""}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "sample_data",
    description: "Get sample records from a dataset. Returns raw market data records.",
    schema: z.object({
      feed: z.string().describe("Feed name (e.g., 'kalshi')"),
      date: z.string().describe("Date YYYY-MM-DD"),
      ticker: z.string().optional().describe("Filter by ticker"),
      type: z.string().optional().describe("Message type: trade, ticker, orderbook"),
      limit: z.number().optional().describe("Max records (default 10)"),
    }),
  }
);

export const getSchema = tool(
  async ({ feed, type }) => {
    const path = `/schema/${feed}/${type}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "get_schema",
    description: "Get schema for a message type. Shows field names, types, and derived fields.",
    schema: z.object({
      feed: z.string().describe("Feed name"),
      type: z.string().describe("Message type: trade, ticker, orderbook"),
    }),
  }
);

export const listBuilders = tool(
  async () => {
    return JSON.stringify(await apiRequest("/builders"));
  },
  {
    name: "list_builders",
    description: "List available state builders for signal development.",
    schema: z.object({}),
  }
);

export const dataTools = [listDatasets, sampleData, getSchema, listBuilders];
```

**Step 2: Verify deno check passes**

Run: `cd ssmd-agent && deno check src/agent/tools.ts`
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/
git commit -m "feat(agent): add data API tools"
```

---

## Task 9: OrderBook State Builder

**Files:**
- Create: `ssmd-agent/src/state/types.ts`
- Create: `ssmd-agent/src/state/orderbook.ts`

**Step 1: Create state types**

```typescript
// ssmd-agent/src/state/types.ts
export interface MarketRecord {
  type: string;
  ticker: string;
  ts: number;
  yes_bid?: number;
  yes_ask?: number;
  no_bid?: number;
  no_ask?: number;
  price?: number;
  count?: number;
  side?: string;
  [key: string]: unknown;
}

export interface StateBuilder<T> {
  id: string;
  update(record: MarketRecord): void;
  getState(): T;
  reset(): void;
}
```

**Step 2: Create OrderBookBuilder**

```typescript
// ssmd-agent/src/state/orderbook.ts
import type { MarketRecord, StateBuilder } from "./types.ts";

export interface OrderBookState {
  ticker: string;
  bestBid: number;
  bestAsk: number;
  spread: number;
  spreadPercent: number;
  lastUpdate: number;
}

export class OrderBookBuilder implements StateBuilder<OrderBookState> {
  id = "orderbook";
  private state: OrderBookState = this.initialState();

  update(record: MarketRecord): void {
    // Only process orderbook or ticker messages
    if (record.type !== "orderbook" && record.type !== "ticker") return;

    const yesBid = record.yes_bid ?? 0;
    const yesAsk = record.yes_ask ?? 0;

    this.state = {
      ticker: record.ticker,
      bestBid: yesBid,
      bestAsk: yesAsk,
      spread: yesAsk - yesBid,
      spreadPercent: yesAsk > 0 ? (yesAsk - yesBid) / yesAsk : 0,
      lastUpdate: record.ts,
    };
  }

  getState(): OrderBookState {
    return { ...this.state };
  }

  reset(): void {
    this.state = this.initialState();
  }

  private initialState(): OrderBookState {
    return {
      ticker: "",
      bestBid: 0,
      bestAsk: 0,
      spread: 0,
      spreadPercent: 0,
      lastUpdate: 0,
    };
  }
}
```

**Step 3: Verify deno check passes**

Run: `cd ssmd-agent && deno check src/state/orderbook.ts`
Expected: No errors

**Step 4: Commit**

```bash
git add ssmd-agent/src/state/
git commit -m "feat(agent): add OrderBookBuilder state builder"
```

---

## Task 10: OrderBook Builder Tool

**Files:**
- Modify: `ssmd-agent/src/agent/tools.ts`

**Step 1: Add orderbook_builder tool**

Add to `ssmd-agent/src/agent/tools.ts`:

```typescript
import { OrderBookBuilder, type OrderBookState } from "../state/orderbook.ts";
import type { MarketRecord } from "../state/types.ts";

export const orderbookBuilder = tool(
  async ({ records }) => {
    const builder = new OrderBookBuilder();
    const snapshots: OrderBookState[] = [];

    for (const record of records as MarketRecord[]) {
      builder.update(record);
      const state = builder.getState();
      // Only add if we have meaningful data
      if (state.ticker) {
        snapshots.push(state);
      }
    }

    return JSON.stringify({
      count: snapshots.length,
      snapshots: snapshots.slice(0, 100), // Limit to prevent huge responses
      summary: snapshots.length > 0 ? {
        ticker: snapshots[0].ticker,
        spreadRange: {
          min: Math.min(...snapshots.map(s => s.spread)),
          max: Math.max(...snapshots.map(s => s.spread)),
        },
      } : null,
    });
  },
  {
    name: "orderbook_builder",
    description: "Process market records through OrderBook state builder. Returns state snapshots with spread calculations.",
    schema: z.object({
      records: z.array(z.any()).describe("Array of market data records from sample_data"),
    }),
  }
);

export const dataTools = [listDatasets, sampleData, getSchema, listBuilders, orderbookBuilder];
```

**Step 2: Verify deno check passes**

Run: `cd ssmd-agent && deno check src/agent/tools.ts`
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/
git commit -m "feat(agent): add orderbook_builder tool"
```

---

## Task 11: Backtest Runner Tool

**Files:**
- Create: `ssmd-agent/src/backtest/runner.ts`
- Modify: `ssmd-agent/src/agent/tools.ts`

**Step 1: Create backtest runner**

```typescript
// ssmd-agent/src/backtest/runner.ts
import type { OrderBookState } from "../state/orderbook.ts";

export interface BacktestResult {
  fires: number;
  errors: string[];
  fireTimes: string[];
  samplePayloads: unknown[];
  recordsProcessed: number;
  durationMs: number;
}

export interface Signal {
  id: string;
  name?: string;
  requires: string[];
  evaluate(state: { orderbook: OrderBookState }): boolean;
  payload(state: { orderbook: OrderBookState }): unknown;
}

export async function compileSignal(code: string): Promise<Signal> {
  // Use data URL for dynamic import in sandbox
  const wrappedCode = `
    ${code}
    export default signal;
  `;
  const dataUrl = `data:text/typescript;base64,${btoa(wrappedCode)}`;

  try {
    const module = await import(dataUrl);
    return module.default as Signal;
  } catch (e) {
    throw new Error(`Signal compilation failed: ${e}`);
  }
}

export async function runBacktest(
  signalCode: string,
  states: OrderBookState[]
): Promise<BacktestResult> {
  const start = Date.now();
  const errors: string[] = [];
  const fires: { time: string; payload: unknown }[] = [];

  let signal: Signal;
  try {
    signal = await compileSignal(signalCode);
  } catch (e) {
    return {
      fires: 0,
      errors: [(e as Error).message],
      fireTimes: [],
      samplePayloads: [],
      recordsProcessed: 0,
      durationMs: Date.now() - start,
    };
  }

  for (const state of states) {
    try {
      const stateMap = { orderbook: state };
      if (signal.evaluate(stateMap)) {
        fires.push({
          time: new Date(state.lastUpdate).toISOString(),
          payload: signal.payload(stateMap),
        });
      }
    } catch (e) {
      errors.push((e as Error).message);
      if (errors.length >= 10) break; // Limit errors
    }
  }

  return {
    fires: fires.length,
    errors,
    fireTimes: fires.slice(0, 20).map((f) => f.time),
    samplePayloads: fires.slice(0, 5).map((f) => f.payload),
    recordsProcessed: states.length,
    durationMs: Date.now() - start,
  };
}
```

**Step 2: Add run_backtest tool**

Add to `ssmd-agent/src/agent/tools.ts`:

```typescript
import { runBacktest as executeBacktest } from "../backtest/runner.ts";

export const runBacktest = tool(
  async ({ signalCode, states }) => {
    const result = await executeBacktest(signalCode, states);
    return JSON.stringify(result);
  },
  {
    name: "run_backtest",
    description: "Evaluate signal code against state snapshots. Returns fire count, errors, and sample payloads.",
    schema: z.object({
      signalCode: z.string().describe("TypeScript signal code with evaluate() and payload() functions"),
      states: z.array(z.any()).describe("OrderBookState snapshots from orderbook_builder"),
    }),
  }
);

export const allTools = [...dataTools, runBacktest];
```

**Step 3: Verify deno check passes**

Run: `cd ssmd-agent && deno check src/backtest/runner.ts src/agent/tools.ts`
Expected: No errors

**Step 4: Commit**

```bash
git add ssmd-agent/
git commit -m "feat(agent): add backtest runner tool"
```

---

## Task 12: Deploy Signal Tool

**Files:**
- Modify: `ssmd-agent/src/agent/tools.ts`

**Step 1: Add deploy_signal tool**

Add to `ssmd-agent/src/agent/tools.ts`:

```typescript
import { config } from "../config.ts";

export const deploySignal = tool(
  async ({ code, path }) => {
    // Ensure path is under signals directory
    const fullPath = `${config.signalsPath}/${path}`;

    // Write the file
    await Deno.writeTextFile(fullPath, code);

    // Git add and commit
    const addCmd = new Deno.Command("git", {
      args: ["add", fullPath],
      stdout: "piped",
      stderr: "piped",
    });
    await addCmd.output();

    const commitCmd = new Deno.Command("git", {
      args: ["commit", "-m", `signal: add ${path}`],
      stdout: "piped",
      stderr: "piped",
    });
    const commitResult = await commitCmd.output();

    if (!commitResult.success) {
      const stderr = new TextDecoder().decode(commitResult.stderr);
      return JSON.stringify({ error: `git commit failed: ${stderr}` });
    }

    // Get commit SHA
    const revCmd = new Deno.Command("git", {
      args: ["rev-parse", "HEAD"],
      stdout: "piped",
    });
    const revResult = await revCmd.output();
    const sha = new TextDecoder().decode(revResult.stdout).trim();

    return JSON.stringify({
      path: fullPath,
      sha: sha.slice(0, 7),
      message: `Deployed to ${fullPath}`,
    });
  },
  {
    name: "deploy_signal",
    description: "Write signal file and git commit. Use after successful backtest.",
    schema: z.object({
      code: z.string().describe("Complete TypeScript signal code"),
      path: z.string().describe("Filename within signals/ directory (e.g., 'spread-alert.ts')"),
    }),
  }
);

export const allTools = [...dataTools, runBacktest, deploySignal];
```

**Step 2: Create signals directory**

Run: `mkdir -p ssmd-agent/signals && touch ssmd-agent/signals/.gitkeep`

**Step 3: Verify deno check passes**

Run: `cd ssmd-agent && deno check src/agent/tools.ts`
Expected: No errors

**Step 4: Commit**

```bash
git add ssmd-agent/
git commit -m "feat(agent): add deploy_signal tool"
```

---

## Task 13: System Prompt Builder

**Files:**
- Create: `ssmd-agent/src/agent/prompt.ts`

**Step 1: Create system prompt builder**

```typescript
// ssmd-agent/src/agent/prompt.ts
import type { Skill } from "./skills.ts";

export function buildSystemPrompt(skills: Skill[]): string {
  const skillsSection = skills.length > 0
    ? skills.map((s) => `### ${s.name}\n${s.description}\n\n${s.content}`).join("\n\n---\n\n")
    : "No skills loaded.";

  return `You are an AI assistant for signal development on the ssmd market data platform.

## Your Role

Help developers create, test, and deploy TypeScript signals that trigger on market conditions. You generate signal code, validate it with backtests, and deploy when ready.

## Available Tools

You have access to tools for:
- **Data discovery**: list_datasets, sample_data, get_schema, list_builders
- **State building**: orderbook_builder (processes records into state snapshots)
- **Validation**: run_backtest (evaluates signal code against states)
- **Deployment**: deploy_signal (writes file and git commits)

## Workflow

1. **Explore data** - Use list_datasets and sample_data to understand what's available
2. **Build state** - Use orderbook_builder to process records into state snapshots
3. **Generate signal** - Write TypeScript code using the Signal interface
4. **Backtest** - Use run_backtest to validate the signal fires appropriately
5. **Iterate** - Adjust thresholds based on fire count (0 = too strict, 1000+ = too loose)
6. **Deploy** - Use deploy_signal when satisfied with backtest results

## Signal Template

\`\`\`typescript
export const signal = {
  id: "my-signal-id",
  name: "Human Readable Name",
  requires: ["orderbook"],

  evaluate(state: { orderbook: OrderBookState }): boolean {
    return state.orderbook.spread > 0.05;
  },

  payload(state: { orderbook: OrderBookState }) {
    return {
      ticker: state.orderbook.ticker,
      spread: state.orderbook.spread,
    };
  },
};
\`\`\`

## Skills

${skillsSection}

## Guidelines

- Always sample data before generating signals to understand the format
- Run backtests before deploying
- Aim for reasonable fire counts (typically 10-100 per day, depends on use case)
- Ask for confirmation before deploying
`;
}
```

**Step 2: Verify deno check passes**

Run: `cd ssmd-agent && deno check src/agent/prompt.ts`
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/
git commit -m "feat(agent): add system prompt builder"
```

---

## Task 14: LangGraph Agent Setup

**Files:**
- Create: `ssmd-agent/src/agent/graph.ts`

**Step 1: Create agent graph**

```typescript
// ssmd-agent/src/agent/graph.ts
import { ChatAnthropic } from "@langchain/anthropic";
import { createReactAgent } from "@langchain/langgraph/prebuilt";
import { config } from "../config.ts";
import { allTools } from "./tools.ts";
import { loadSkills } from "./skills.ts";
import { buildSystemPrompt } from "./prompt.ts";

export async function createAgent() {
  const model = new ChatAnthropic({
    model: config.model,
    anthropicApiKey: config.anthropicApiKey,
  });

  const skills = await loadSkills();
  const systemPrompt = buildSystemPrompt(skills);

  const agent = createReactAgent({
    llm: model,
    tools: allTools,
    messageModifier: systemPrompt,
  });

  return agent;
}
```

**Step 2: Verify deno check passes**

Run: `cd ssmd-agent && deno check src/agent/graph.ts`
Expected: No errors (may need to run deno cache first)

**Step 3: Commit**

```bash
git add ssmd-agent/
git commit -m "feat(agent): add LangGraph agent setup"
```

---

## Task 15: Streaming CLI REPL

**Files:**
- Create: `ssmd-agent/src/cli.ts`

**Step 1: Create CLI with streaming**

```typescript
// ssmd-agent/src/cli.ts
import { validateConfig } from "./config.ts";
import { createAgent } from "./agent/graph.ts";

function formatArgs(input: unknown): string {
  if (typeof input === "object" && input !== null) {
    const obj = input as Record<string, unknown>;
    const parts = Object.entries(obj)
      .filter(([_, v]) => v !== undefined)
      .map(([k, v]) => `${k}=${JSON.stringify(v)}`);
    return parts.join(", ");
  }
  return String(input);
}

function formatResult(output: unknown): string {
  if (typeof output === "string") {
    try {
      const parsed = JSON.parse(output);
      if (Array.isArray(parsed)) {
        return `${parsed.length} items`;
      }
      if (parsed.count !== undefined) {
        return `${parsed.count} snapshots`;
      }
      if (parsed.fires !== undefined) {
        return `${parsed.fires} fires, ${parsed.errors?.length ?? 0} errors`;
      }
      if (parsed.sha) {
        return `Committed: ${parsed.sha}`;
      }
      return output.slice(0, 100) + (output.length > 100 ? "..." : "");
    } catch {
      return output.slice(0, 100) + (output.length > 100 ? "..." : "");
    }
  }
  return String(output);
}

async function main() {
  try {
    validateConfig();
  } catch (e) {
    console.error((e as Error).message);
    Deno.exit(1);
  }

  console.log("ssmd-agent v0.1.0");
  console.log("Type 'quit' to exit\n");

  const agent = await createAgent();
  const encoder = new TextEncoder();

  while (true) {
    const input = prompt("ssmd-agent>");
    if (!input || input === "quit" || input === "exit") {
      console.log("Goodbye!");
      break;
    }

    try {
      for await (const event of agent.streamEvents(
        { messages: [{ role: "user", content: input }] },
        { version: "v2" }
      )) {
        switch (event.event) {
          case "on_chat_model_stream": {
            const chunk = event.data?.chunk;
            if (chunk?.content) {
              Deno.stdout.writeSync(encoder.encode(chunk.content));
            }
            break;
          }
          case "on_tool_start": {
            console.log(`\n[tool] ${event.name}(${formatArgs(event.data?.input)})`);
            break;
          }
          case "on_tool_end": {
            console.log(`  → ${formatResult(event.data?.output)}`);
            break;
          }
        }
      }
      console.log("\n");
    } catch (e) {
      console.error(`\nError: ${(e as Error).message}\n`);
    }
  }
}

main();
```

**Step 2: Verify deno check passes**

Run: `cd ssmd-agent && deno check src/cli.ts`
Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/
git commit -m "feat(agent): add streaming CLI REPL"
```

---

## Task 16: Additional Skills

**Files:**
- Create: `ssmd-agent/skills/monitor-spread.md`
- Create: `ssmd-agent/skills/interpret-backtest.md`
- Create: `ssmd-agent/skills/custom-signal.md`

**Step 1: Create monitor-spread skill**

```markdown
---
name: monitor-spread
description: Generate spread monitoring signals for prediction markets
---

# Spread Monitoring Signals

Use when user wants alerts on bid-ask spread widening.

## Workflow

1. `sample_data` with type="orderbook" to get orderbook records
2. `orderbook_builder` to see spread distribution
3. Generate signal with appropriate threshold
4. `run_backtest` to validate fire frequency
5. Adjust threshold if needed

## Template

\`\`\`typescript
export const signal = {
  id: "{{ticker}}-spread-alert",
  name: "{{ticker}} Spread Alert",
  requires: ["orderbook"],

  evaluate(state: { orderbook: OrderBookState }): boolean {
    return state.orderbook.ticker.startsWith("{{ticker}}")
        && state.orderbook.spreadPercent > {{threshold}};
  },

  payload(state: { orderbook: OrderBookState }) {
    return {
      ticker: state.orderbook.ticker,
      spread: state.orderbook.spread,
      spreadPercent: state.orderbook.spreadPercent,
      bestBid: state.orderbook.bestBid,
      bestAsk: state.orderbook.bestAsk,
    };
  },
};
\`\`\`

## Thresholds

- 0.03 (3%): Catches most spread widening, may be noisy
- 0.05 (5%): Good default for prediction markets
- 0.10 (10%): Only catches significant events
```

**Step 2: Create interpret-backtest skill**

```markdown
---
name: interpret-backtest
description: How to analyze backtest results
---

# Interpreting Backtest Results

## Key Metrics

- **fires**: Number of times signal triggered
- **errors**: Runtime errors in signal code
- **fireTimes**: When signals fired (check clustering)
- **samplePayloads**: Example payloads (verify data looks right)

## Fire Count Guidelines

| Count | Interpretation | Action |
|-------|----------------|--------|
| 0 | Condition never met | Loosen threshold |
| 1-10 | Rare events | May be appropriate for alerts |
| 10-100 | Moderate frequency | Good for daily monitoring |
| 100-500 | Frequent | Consider if this is too noisy |
| 500+ | Very frequent | Likely needs tighter conditions |

## Common Issues

- **fires: 0, errors: 0**: Threshold too strict, or data doesn't have expected pattern
- **fires: 0, errors: [...]**: Bug in signal code, check error messages
- **Clustered fireTimes**: Signal fires rapidly then stops - may need cooldown
- **All same payload values**: Signal may not be updating state correctly
```

**Step 3: Create custom-signal skill**

```markdown
---
name: custom-signal
description: Template for custom signal logic
---

# Custom Signals

For signals that don't fit standard templates.

## Signal Interface

\`\`\`typescript
export const signal = {
  id: string,           // Unique kebab-case identifier
  name: string,         // Human-readable name
  requires: string[],   // State builders needed: ["orderbook"]

  evaluate(state): boolean,  // Return true to fire
  payload(state): object,    // Data to include when fired
};
\`\`\`

## State Fields

### OrderBook (state.orderbook)
- ticker: string
- bestBid: number
- bestAsk: number
- spread: number (ask - bid)
- spreadPercent: number (spread / ask)
- lastUpdate: number (Unix ms)

## Combining Conditions

\`\`\`typescript
evaluate(state) {
  const book = state.orderbook;
  return book.spread > 0.05
      && book.ticker.startsWith("INXD")
      && book.bestBid > 0.20;
}
\`\`\`

## Adding Cooldown (manual tracking)

\`\`\`typescript
let lastFire = 0;
const COOLDOWN_MS = 60000; // 1 minute

evaluate(state) {
  if (Date.now() - lastFire < COOLDOWN_MS) return false;
  if (state.orderbook.spread > 0.05) {
    lastFire = Date.now();
    return true;
  }
  return false;
}
\`\`\`
```

**Step 4: Commit**

```bash
git add ssmd-agent/skills/
git commit -m "feat(agent): add monitor-spread, interpret-backtest, custom-signal skills"
```

---

## Task 17: Integration Test

**Files:**
- Create: `ssmd-agent/test/integration.ts`

**Step 1: Create basic integration test**

```typescript
// ssmd-agent/test/integration.ts
import { assertEquals } from "https://deno.land/std@0.208.0/assert/mod.ts";
import { OrderBookBuilder } from "../src/state/orderbook.ts";
import { loadSkills } from "../src/agent/skills.ts";

Deno.test("OrderBookBuilder calculates spread", () => {
  const builder = new OrderBookBuilder();

  builder.update({
    type: "orderbook",
    ticker: "INXD-25001",
    ts: 1735084800000,
    yes_bid: 0.45,
    yes_ask: 0.55,
  });

  const state = builder.getState();
  assertEquals(state.ticker, "INXD-25001");
  assertEquals(state.bestBid, 0.45);
  assertEquals(state.bestAsk, 0.55);
  assertEquals(state.spread, 0.1);
});

Deno.test("Skills loader finds skills", async () => {
  const skills = await loadSkills();
  const names = skills.map((s) => s.name);

  assertEquals(names.includes("explore-data"), true);
  assertEquals(names.includes("monitor-spread"), true);
});
```

**Step 2: Run tests**

Run: `cd ssmd-agent && deno test test/integration.ts --allow-read`
Expected: PASS

**Step 3: Add test task to deno.json**

Update `ssmd-agent/deno.json`:

```json
{
  "tasks": {
    "start": "deno run --allow-net --allow-env src/main.ts",
    "agent": "deno run --allow-net --allow-env --allow-read --allow-write --allow-run src/cli.ts",
    "dev": "deno run --watch --allow-net --allow-env src/main.ts",
    "check": "deno check src/main.ts src/cli.ts",
    "test": "deno test --allow-read --allow-net test/"
  },
  ...
}
```

**Step 4: Commit**

```bash
git add ssmd-agent/
git commit -m "test(agent): add integration tests"
```

---

## Task 18: Update Makefile

**Files:**
- Modify: `Makefile`

**Step 1: Add ssmd-data and agent targets**

Add to `Makefile`:

```makefile
# ssmd-data targets
.PHONY: data-build data-test data-run

data-build:
	go build -o bin/ssmd-data ./cmd/ssmd-data

data-test:
	go test ./internal/api/... -v

data-run: data-build
	SSMD_DATA_PATH=./testdata SSMD_API_KEY=dev ./bin/ssmd-data

# ssmd-agent targets
.PHONY: agent-check agent-test agent-run

agent-check:
	cd ssmd-agent && deno check src/main.ts src/cli.ts

agent-test:
	cd ssmd-agent && deno test --allow-read --allow-net test/

agent-run:
	cd ssmd-agent && deno task agent
```

**Step 2: Test targets**

Run: `make data-build`
Expected: Binary created at bin/ssmd-data

Run: `make agent-check`
Expected: No errors

**Step 3: Commit**

```bash
git add Makefile
git commit -m "build: add ssmd-data and agent Makefile targets"
```

---

## Task 19: Update TODO.md

**Files:**
- Modify: `TODO.md`

**Step 1: Update Phase 3 status**

Mark completed items in Phase 3:

```markdown
### Phase 3: Agent Pipeline MVP
Design: `docs/plans/2025-12-25-agent-pipeline-mvp-design.md`
Depends on: Phase 2 Archiver (provides JSONL data files)

**ssmd-data API (Go):**
- [x] HTTP server skeleton with health endpoint
- [x] /datasets endpoint with filtering
- [x] /datasets/{feed}/{date}/sample endpoint
- [x] /schema and /builders endpoints
- [x] API key authentication middleware
- [x] Dockerfile

**ssmd-agent REPL (Deno):**
- [x] LangGraph dependencies and config
- [x] Skills loader from markdown
- [x] Data tools (list_datasets, sample_data, get_schema, list_builders)
- [x] OrderBookBuilder state builder
- [x] orderbook_builder tool
- [x] Backtest runner tool
- [x] deploy_signal tool
- [x] System prompt builder
- [x] LangGraph agent setup
- [x] Streaming CLI REPL

**Skills (Markdown):**
- [x] explore-data
- [x] monitor-spread
- [x] interpret-backtest
- [x] custom-signal
```

**Step 2: Commit**

```bash
git add TODO.md
git commit -m "docs: mark agent pipeline MVP tasks complete"
```

---

## Summary

**19 tasks** covering:

1. ssmd-data server skeleton
2. /datasets endpoint
3. /sample endpoint
4. /schema and /builders endpoints
5. Dockerfile
6. Agent dependencies and config
7. Skills loader
8. Data tools
9. OrderBookBuilder
10. orderbook_builder tool
11. Backtest runner tool
12. Deploy signal tool
13. System prompt builder
14. LangGraph agent setup
15. Streaming CLI REPL
16. Additional skills
17. Integration tests
18. Makefile updates
19. TODO.md updates

Each task follows TDD where applicable (write test, verify fail, implement, verify pass, commit).
