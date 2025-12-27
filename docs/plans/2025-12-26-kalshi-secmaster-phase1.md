# Kalshi Secmaster Phase 1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add secmaster (events/markets/fees) to ssmd with sync from Kalshi API, database storage, API endpoints, and agent tools.

**Architecture:** Go-based sync job fetches from Kalshi REST API and stores in PostgreSQL. ssmd-data API exposes `/markets`, `/events`, `/fees` endpoints. Agent tools call these endpoints.

**Tech Stack:** Go 1.23, PostgreSQL, sqlx, ssmd-data HTTP API, Deno/TypeScript agent tools

---

## Task 1: Add PostgreSQL Schema Types

**Files:**
- Create: `internal/types/secmaster.go`

**Step 1: Create secmaster types**

```go
// internal/types/secmaster.go
package types

import "time"

// MarketStatus represents the status of a market
type MarketStatus string

const (
	MarketStatusOpen    MarketStatus = "open"
	MarketStatusClosed  MarketStatus = "closed"
	MarketStatusSettled MarketStatus = "settled"
)

// Event represents a Kalshi event (container for markets)
type Event struct {
	EventTicker       string     `json:"event_ticker" db:"event_ticker"`
	Title             string     `json:"title" db:"title"`
	Category          string     `json:"category" db:"category"`
	SeriesTicker      string     `json:"series_ticker" db:"series_ticker"`
	StrikeDate        *time.Time `json:"strike_date,omitempty" db:"strike_date"`
	MutuallyExclusive bool       `json:"mutually_exclusive" db:"mutually_exclusive"`
	Status            string     `json:"status" db:"status"`
	CreatedAt         time.Time  `json:"created_at" db:"created_at"`
	UpdatedAt         time.Time  `json:"updated_at" db:"updated_at"`
}

// Market represents a Kalshi market (tradeable contract)
type Market struct {
	Ticker       string       `json:"ticker" db:"ticker"`
	EventTicker  string       `json:"event_ticker" db:"event_ticker"`
	Title        string       `json:"title" db:"title"`
	Status       MarketStatus `json:"status" db:"status"`
	CloseTime    *time.Time   `json:"close_time,omitempty" db:"close_time"`
	YesBid       *int         `json:"yes_bid,omitempty" db:"yes_bid"`
	YesAsk       *int         `json:"yes_ask,omitempty" db:"yes_ask"`
	NoBid        *int         `json:"no_bid,omitempty" db:"no_bid"`
	NoAsk        *int         `json:"no_ask,omitempty" db:"no_ask"`
	LastPrice    *int         `json:"last_price,omitempty" db:"last_price"`
	Volume       *int64       `json:"volume,omitempty" db:"volume"`
	Volume24h    *int64       `json:"volume_24h,omitempty" db:"volume_24h"`
	OpenInterest *int64       `json:"open_interest,omitempty" db:"open_interest"`
	CreatedAt    time.Time    `json:"created_at" db:"created_at"`
	UpdatedAt    time.Time    `json:"updated_at" db:"updated_at"`
}

// MarketWithEvent is a market joined with event metadata
type MarketWithEvent struct {
	Market
	Category     string `json:"category" db:"category"`
	SeriesTicker string `json:"series_ticker" db:"series_ticker"`
	EventTitle   string `json:"event_title" db:"event_title"`
}

// Fee represents fee schedule for a tier
type Fee struct {
	Tier      string  `json:"tier" db:"tier"`
	MakerFee  float64 `json:"maker_fee" db:"maker_fee"`
	TakerFee  float64 `json:"taker_fee" db:"taker_fee"`
	UpdatedAt time.Time `json:"updated_at" db:"updated_at"`
}
```

**Step 2: Verify build**

Run: `cd /workspaces/ssmd && make build`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add internal/types/secmaster.go
git commit -m "feat(types): add secmaster types for events, markets, fees"
```

---

## Task 2: Add Database Migration

**Files:**
- Create: `migrations/001_secmaster.sql`

**Step 1: Create migrations directory and schema**

```sql
-- migrations/001_secmaster.sql
-- Kalshi secmaster schema

CREATE TABLE IF NOT EXISTS events (
    event_ticker VARCHAR(64) PRIMARY KEY,
    title TEXT NOT NULL,
    category VARCHAR(64) NOT NULL DEFAULT '',
    series_ticker VARCHAR(64) NOT NULL DEFAULT '',
    strike_date TIMESTAMPTZ,
    mutually_exclusive BOOLEAN NOT NULL DEFAULT false,
    status VARCHAR(16) NOT NULL DEFAULT 'open',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX idx_events_category ON events(category) WHERE deleted_at IS NULL;
CREATE INDEX idx_events_series ON events(series_ticker) WHERE deleted_at IS NULL;
CREATE INDEX idx_events_status ON events(status) WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS markets (
    ticker VARCHAR(64) PRIMARY KEY,
    event_ticker VARCHAR(64) NOT NULL REFERENCES events(event_ticker),
    title TEXT NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'open',
    close_time TIMESTAMPTZ,
    yes_bid INT,
    yes_ask INT,
    no_bid INT,
    no_ask INT,
    last_price INT,
    volume BIGINT,
    volume_24h BIGINT,
    open_interest BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX idx_markets_event ON markets(event_ticker) WHERE deleted_at IS NULL;
CREATE INDEX idx_markets_status ON markets(status) WHERE deleted_at IS NULL;
CREATE INDEX idx_markets_close_time ON markets(close_time) WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS fees (
    tier VARCHAR(32) PRIMARY KEY,
    maker_fee DECIMAL(6,4) NOT NULL,
    taker_fee DECIMAL(6,4) NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Insert default fee tier
INSERT INTO fees (tier, maker_fee, taker_fee) VALUES ('default', 0.07, 0.07)
ON CONFLICT (tier) DO NOTHING;
```

**Step 2: Commit**

```bash
git add migrations/001_secmaster.sql
git commit -m "feat(db): add secmaster schema migration"
```

---

## Task 3: Add Secmaster Store

**Files:**
- Create: `internal/secmaster/store.go`

**Step 1: Create store with CRUD operations**

```go
// internal/secmaster/store.go
package secmaster

import (
	"context"
	"database/sql"
	"time"

	"github.com/aaronwald/ssmd/internal/types"
)

// Store handles secmaster database operations
type Store struct {
	db *sql.DB
}

// NewStore creates a new secmaster store
func NewStore(db *sql.DB) *Store {
	return &Store{db: db}
}

// UpsertEvent inserts or updates an event
func (s *Store) UpsertEvent(ctx context.Context, e *types.Event) error {
	query := `
		INSERT INTO events (event_ticker, title, category, series_ticker, strike_date, mutually_exclusive, status, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
		ON CONFLICT (event_ticker) DO UPDATE SET
			title = EXCLUDED.title,
			category = EXCLUDED.category,
			series_ticker = EXCLUDED.series_ticker,
			strike_date = EXCLUDED.strike_date,
			mutually_exclusive = EXCLUDED.mutually_exclusive,
			status = EXCLUDED.status,
			updated_at = NOW(),
			deleted_at = NULL
	`
	_, err := s.db.ExecContext(ctx, query,
		e.EventTicker, e.Title, e.Category, e.SeriesTicker,
		e.StrikeDate, e.MutuallyExclusive, e.Status)
	return err
}

// UpsertMarket inserts or updates a market
func (s *Store) UpsertMarket(ctx context.Context, m *types.Market) error {
	query := `
		INSERT INTO markets (ticker, event_ticker, title, status, close_time,
			yes_bid, yes_ask, no_bid, no_ask, last_price, volume, volume_24h, open_interest, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, NOW())
		ON CONFLICT (ticker) DO UPDATE SET
			title = EXCLUDED.title,
			status = EXCLUDED.status,
			close_time = EXCLUDED.close_time,
			yes_bid = EXCLUDED.yes_bid,
			yes_ask = EXCLUDED.yes_ask,
			no_bid = EXCLUDED.no_bid,
			no_ask = EXCLUDED.no_ask,
			last_price = EXCLUDED.last_price,
			volume = EXCLUDED.volume,
			volume_24h = EXCLUDED.volume_24h,
			open_interest = EXCLUDED.open_interest,
			updated_at = NOW(),
			deleted_at = NULL
	`
	_, err := s.db.ExecContext(ctx, query,
		m.Ticker, m.EventTicker, m.Title, m.Status, m.CloseTime,
		m.YesBid, m.YesAsk, m.NoBid, m.NoAsk, m.LastPrice,
		m.Volume, m.Volume24h, m.OpenInterest)
	return err
}

// ListMarkets returns markets with optional filters
func (s *Store) ListMarkets(ctx context.Context, opts MarketListOptions) ([]types.MarketWithEvent, error) {
	query := `
		SELECT m.ticker, m.event_ticker, m.title, m.status, m.close_time,
			m.yes_bid, m.yes_ask, m.no_bid, m.no_ask, m.last_price,
			m.volume, m.volume_24h, m.open_interest, m.created_at, m.updated_at,
			e.category, e.series_ticker, e.title as event_title
		FROM markets m
		JOIN events e ON m.event_ticker = e.event_ticker
		WHERE m.deleted_at IS NULL AND e.deleted_at IS NULL
	`
	args := []interface{}{}
	argNum := 1

	if opts.Category != "" {
		query += ` AND e.category = $` + string(rune('0'+argNum))
		args = append(args, opts.Category)
		argNum++
	}
	if opts.Status != "" {
		query += ` AND m.status = $` + string(rune('0'+argNum))
		args = append(args, opts.Status)
		argNum++
	}
	if opts.Series != "" {
		query += ` AND e.series_ticker = $` + string(rune('0'+argNum))
		args = append(args, opts.Series)
		argNum++
	}
	if opts.ClosingBefore != nil {
		query += ` AND m.close_time < $` + string(rune('0'+argNum))
		args = append(args, opts.ClosingBefore)
		argNum++
	}
	if opts.ClosingAfter != nil {
		query += ` AND m.close_time > $` + string(rune('0'+argNum))
		args = append(args, opts.ClosingAfter)
		argNum++
	}

	query += ` ORDER BY m.close_time ASC NULLS LAST`

	if opts.Limit > 0 {
		query += ` LIMIT $` + string(rune('0'+argNum))
		args = append(args, opts.Limit)
	}

	rows, err := s.db.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var markets []types.MarketWithEvent
	for rows.Next() {
		var m types.MarketWithEvent
		err := rows.Scan(
			&m.Ticker, &m.EventTicker, &m.Title, &m.Status, &m.CloseTime,
			&m.YesBid, &m.YesAsk, &m.NoBid, &m.NoAsk, &m.LastPrice,
			&m.Volume, &m.Volume24h, &m.OpenInterest, &m.CreatedAt, &m.UpdatedAt,
			&m.Category, &m.SeriesTicker, &m.EventTitle,
		)
		if err != nil {
			return nil, err
		}
		markets = append(markets, m)
	}
	return markets, rows.Err()
}

// GetMarket returns a single market by ticker
func (s *Store) GetMarket(ctx context.Context, ticker string) (*types.MarketWithEvent, error) {
	query := `
		SELECT m.ticker, m.event_ticker, m.title, m.status, m.close_time,
			m.yes_bid, m.yes_ask, m.no_bid, m.no_ask, m.last_price,
			m.volume, m.volume_24h, m.open_interest, m.created_at, m.updated_at,
			e.category, e.series_ticker, e.title as event_title
		FROM markets m
		JOIN events e ON m.event_ticker = e.event_ticker
		WHERE m.ticker = $1 AND m.deleted_at IS NULL
	`
	var m types.MarketWithEvent
	err := s.db.QueryRowContext(ctx, query, ticker).Scan(
		&m.Ticker, &m.EventTicker, &m.Title, &m.Status, &m.CloseTime,
		&m.YesBid, &m.YesAsk, &m.NoBid, &m.NoAsk, &m.LastPrice,
		&m.Volume, &m.Volume24h, &m.OpenInterest, &m.CreatedAt, &m.UpdatedAt,
		&m.Category, &m.SeriesTicker, &m.EventTitle,
	)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	return &m, err
}

// GetFees returns fees for a tier
func (s *Store) GetFees(ctx context.Context, tier string) (*types.Fee, error) {
	query := `SELECT tier, maker_fee, taker_fee, updated_at FROM fees WHERE tier = $1`
	var f types.Fee
	err := s.db.QueryRowContext(ctx, query, tier).Scan(&f.Tier, &f.MakerFee, &f.TakerFee, &f.UpdatedAt)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	return &f, err
}

// MarketListOptions for filtering markets
type MarketListOptions struct {
	Category      string
	Status        string
	Series        string
	ClosingBefore *time.Time
	ClosingAfter  *time.Time
	Limit         int
}
```

**Step 2: Verify build**

Run: `cd /workspaces/ssmd && make build`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add internal/secmaster/store.go
git commit -m "feat(secmaster): add database store for markets/events/fees"
```

---

## Task 4: Add Kalshi API Client

**Files:**
- Create: `internal/secmaster/kalshi.go`

**Step 1: Create minimal Kalshi REST client**

```go
// internal/secmaster/kalshi.go
package secmaster

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"time"

	"github.com/aaronwald/ssmd/internal/types"
)

const (
	KalshiAPIBase     = "https://api.elections.kalshi.com/trade-api/v2"
	DefaultPageLimit  = 200
	RequestTimeout    = 30 * time.Second
)

// KalshiClient for REST API calls
type KalshiClient struct {
	baseURL    string
	httpClient *http.Client
}

// NewKalshiClient creates a new client
func NewKalshiClient() *KalshiClient {
	return &KalshiClient{
		baseURL:    KalshiAPIBase,
		httpClient: &http.Client{Timeout: RequestTimeout},
	}
}

// EventsResponse from Kalshi API
type EventsResponse struct {
	Events []KalshiEvent `json:"events"`
	Cursor string        `json:"cursor"`
}

// KalshiEvent from API
type KalshiEvent struct {
	EventTicker       string  `json:"event_ticker"`
	Title             string  `json:"title"`
	Category          string  `json:"category"`
	SeriesTicker      string  `json:"series_ticker"`
	StrikeDate        *string `json:"strike_date"`
	MutuallyExclusive bool    `json:"mutually_exclusive"`
}

// MarketsResponse from Kalshi API
type MarketsResponse struct {
	Markets []KalshiMarket `json:"markets"`
	Cursor  string         `json:"cursor"`
}

// KalshiMarket from API
type KalshiMarket struct {
	Ticker       string  `json:"ticker"`
	EventTicker  string  `json:"event_ticker"`
	Title        string  `json:"title"`
	Status       string  `json:"status"`
	CloseTime    *string `json:"close_time"`
	YesBid       *int    `json:"yes_bid"`
	YesAsk       *int    `json:"yes_ask"`
	NoBid        *int    `json:"no_bid"`
	NoAsk        *int    `json:"no_ask"`
	LastPrice    *int    `json:"last_price"`
	Volume       *int64  `json:"volume"`
	Volume24h    *int64  `json:"volume_24h"`
	OpenInterest *int64  `json:"open_interest"`
}

// FetchAllEvents fetches all events with pagination
func (c *KalshiClient) FetchAllEvents(minCloseTS int64) ([]types.Event, error) {
	var allEvents []types.Event
	cursor := ""

	for {
		params := url.Values{}
		params.Set("limit", fmt.Sprintf("%d", DefaultPageLimit))
		if minCloseTS > 0 {
			params.Set("min_close_ts", fmt.Sprintf("%d", minCloseTS))
		}
		if cursor != "" {
			params.Set("cursor", cursor)
		}

		url := fmt.Sprintf("%s/events?%s", c.baseURL, params.Encode())
		resp, err := c.httpClient.Get(url)
		if err != nil {
			return nil, fmt.Errorf("fetch events: %w", err)
		}
		defer resp.Body.Close()

		if resp.StatusCode != http.StatusOK {
			return nil, fmt.Errorf("events API returned %d", resp.StatusCode)
		}

		var result EventsResponse
		if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
			return nil, fmt.Errorf("decode events: %w", err)
		}

		for _, e := range result.Events {
			event := types.Event{
				EventTicker:       e.EventTicker,
				Title:             e.Title,
				Category:          e.Category,
				SeriesTicker:      e.SeriesTicker,
				MutuallyExclusive: e.MutuallyExclusive,
				Status:            "open",
			}
			if e.StrikeDate != nil {
				t, _ := time.Parse(time.RFC3339, *e.StrikeDate)
				event.StrikeDate = &t
			}
			allEvents = append(allEvents, event)
		}

		if result.Cursor == "" {
			break
		}
		cursor = result.Cursor
		time.Sleep(250 * time.Millisecond) // Rate limit
	}

	return allEvents, nil
}

// FetchAllMarkets fetches all markets with pagination
func (c *KalshiClient) FetchAllMarkets(minCloseTS int64) ([]types.Market, error) {
	var allMarkets []types.Market
	cursor := ""

	for {
		params := url.Values{}
		params.Set("limit", "1000")
		if minCloseTS > 0 {
			params.Set("min_close_ts", fmt.Sprintf("%d", minCloseTS))
		}
		if cursor != "" {
			params.Set("cursor", cursor)
		}

		url := fmt.Sprintf("%s/markets?%s", c.baseURL, params.Encode())
		resp, err := c.httpClient.Get(url)
		if err != nil {
			return nil, fmt.Errorf("fetch markets: %w", err)
		}
		defer resp.Body.Close()

		if resp.StatusCode != http.StatusOK {
			return nil, fmt.Errorf("markets API returned %d", resp.StatusCode)
		}

		var result MarketsResponse
		if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
			return nil, fmt.Errorf("decode markets: %w", err)
		}

		for _, m := range result.Markets {
			market := types.Market{
				Ticker:       m.Ticker,
				EventTicker:  m.EventTicker,
				Title:        m.Title,
				Status:       types.MarketStatus(m.Status),
				YesBid:       m.YesBid,
				YesAsk:       m.YesAsk,
				NoBid:        m.NoBid,
				NoAsk:        m.NoAsk,
				LastPrice:    m.LastPrice,
				Volume:       m.Volume,
				Volume24h:    m.Volume24h,
				OpenInterest: m.OpenInterest,
			}
			if m.CloseTime != nil {
				t, _ := time.Parse(time.RFC3339, *m.CloseTime)
				market.CloseTime = &t
			}
			allMarkets = append(allMarkets, market)
		}

		if result.Cursor == "" {
			break
		}
		cursor = result.Cursor
		time.Sleep(250 * time.Millisecond) // Rate limit
	}

	return allMarkets, nil
}
```

**Step 2: Verify build**

Run: `cd /workspaces/ssmd && make build`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add internal/secmaster/kalshi.go
git commit -m "feat(secmaster): add Kalshi REST API client"
```

---

## Task 5: Add Sync Command

**Files:**
- Create: `internal/cmd/secmaster.go`

**Step 1: Create sync command**

```go
// internal/cmd/secmaster.go
package cmd

import (
	"context"
	"database/sql"
	"fmt"
	"os"
	"time"

	"github.com/aaronwald/ssmd/internal/secmaster"
	"github.com/spf13/cobra"
	_ "github.com/lib/pq"
)

var secmasterCmd = &cobra.Command{
	Use:   "secmaster",
	Short: "Manage security master data",
}

var syncCmd = &cobra.Command{
	Use:   "sync",
	Short: "Sync events and markets from Kalshi",
	RunE:  runSync,
}

var (
	syncIncremental bool
)

func init() {
	syncCmd.Flags().BoolVar(&syncIncremental, "incremental", false, "Incremental sync (1-day window)")
	secmasterCmd.AddCommand(syncCmd)
}

func SecmasterCommand() *cobra.Command {
	return secmasterCmd
}

func runSync(cmd *cobra.Command, args []string) error {
	dbURL := os.Getenv("DATABASE_URL")
	if dbURL == "" {
		return fmt.Errorf("DATABASE_URL required")
	}

	db, err := sql.Open("postgres", dbURL)
	if err != nil {
		return fmt.Errorf("connect to database: %w", err)
	}
	defer db.Close()

	store := secmaster.NewStore(db)
	client := secmaster.NewKalshiClient()

	// Calculate min_close_ts (30 days for full, 1 day for incremental)
	windowDays := 30
	if syncIncremental {
		windowDays = 1
	}
	minCloseTS := time.Now().Add(-time.Duration(windowDays) * 24 * time.Hour).Unix()

	ctx := context.Background()

	// Fetch and upsert events
	fmt.Printf("Fetching events (window: %d days)...\n", windowDays)
	events, err := client.FetchAllEvents(minCloseTS)
	if err != nil {
		return fmt.Errorf("fetch events: %w", err)
	}
	fmt.Printf("Fetched %d events\n", len(events))

	for _, e := range events {
		if err := store.UpsertEvent(ctx, &e); err != nil {
			return fmt.Errorf("upsert event %s: %w", e.EventTicker, err)
		}
	}

	// Fetch and upsert markets
	fmt.Printf("Fetching markets...\n")
	markets, err := client.FetchAllMarkets(minCloseTS)
	if err != nil {
		return fmt.Errorf("fetch markets: %w", err)
	}
	fmt.Printf("Fetched %d markets\n", len(markets))

	for _, m := range markets {
		if err := store.UpsertMarket(ctx, &m); err != nil {
			return fmt.Errorf("upsert market %s: %w", m.Ticker, err)
		}
	}

	fmt.Printf("Sync complete: %d events, %d markets\n", len(events), len(markets))
	return nil
}
```

**Step 2: Register command in root**

Modify: `cmd/ssmd/main.go` - add `rootCmd.AddCommand(cmd.SecmasterCommand())`

**Step 3: Verify build**

Run: `cd /workspaces/ssmd && make build`
Expected: Build succeeds

**Step 4: Commit**

```bash
git add internal/cmd/secmaster.go cmd/ssmd/main.go
git commit -m "feat(cli): add ssmd secmaster sync command"
```

---

## Task 6: Add API Handlers for Markets/Events/Fees

**Files:**
- Modify: `internal/api/handlers.go`
- Modify: `internal/api/server.go`

**Step 1: Add secmaster handlers**

Add to `internal/api/handlers.go`:

```go
func (s *Server) handleMarkets(w http.ResponseWriter, r *http.Request) {
	if s.secmasterStore == nil {
		http.Error(w, `{"error":"secmaster not configured"}`, http.StatusServiceUnavailable)
		return
	}

	opts := secmaster.MarketListOptions{
		Category: r.URL.Query().Get("category"),
		Status:   r.URL.Query().Get("status"),
		Series:   r.URL.Query().Get("series"),
		Limit:    100,
	}

	if limitStr := r.URL.Query().Get("limit"); limitStr != "" {
		if l, err := strconv.Atoi(limitStr); err == nil && l > 0 {
			opts.Limit = l
		}
	}

	if before := r.URL.Query().Get("closing_before"); before != "" {
		if t, err := time.Parse(time.RFC3339, before); err == nil {
			opts.ClosingBefore = &t
		}
	}

	if after := r.URL.Query().Get("closing_after"); after != "" {
		if t, err := time.Parse(time.RFC3339, after); err == nil {
			opts.ClosingAfter = &t
		}
	}

	markets, err := s.secmasterStore.ListMarkets(r.Context(), opts)
	if err != nil {
		http.Error(w, `{"error":"query failed"}`, http.StatusInternalServerError)
		return
	}

	if markets == nil {
		markets = []types.MarketWithEvent{}
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(markets)
}

func (s *Server) handleMarket(w http.ResponseWriter, r *http.Request) {
	ticker := r.PathValue("ticker")

	market, err := s.secmasterStore.GetMarket(r.Context(), ticker)
	if err != nil {
		http.Error(w, `{"error":"query failed"}`, http.StatusInternalServerError)
		return
	}
	if market == nil {
		http.Error(w, `{"error":"market not found"}`, http.StatusNotFound)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(market)
}

func (s *Server) handleFees(w http.ResponseWriter, r *http.Request) {
	tier := r.URL.Query().Get("tier")
	if tier == "" {
		tier = "default"
	}

	fees, err := s.secmasterStore.GetFees(r.Context(), tier)
	if err != nil {
		http.Error(w, `{"error":"query failed"}`, http.StatusInternalServerError)
		return
	}
	if fees == nil {
		http.Error(w, `{"error":"tier not found"}`, http.StatusNotFound)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(fees)
}
```

**Step 2: Add routes**

Update `internal/api/server.go` routes():

```go
s.mux.HandleFunc("GET /markets", s.requireAPIKey(s.handleMarkets))
s.mux.HandleFunc("GET /markets/{ticker}", s.requireAPIKey(s.handleMarket))
s.mux.HandleFunc("GET /fees", s.requireAPIKey(s.handleFees))
```

**Step 3: Add secmasterStore to Server struct**

Update Server struct and NewServer to accept optional secmaster store.

**Step 4: Verify build**

Run: `cd /workspaces/ssmd && make build`
Expected: Build succeeds

**Step 5: Commit**

```bash
git add internal/api/handlers.go internal/api/server.go
git commit -m "feat(api): add /markets, /markets/{ticker}, /fees endpoints"
```

---

## Task 7: Add Agent Tools

**Files:**
- Modify: `ssmd-agent/src/agent/tools.ts`

**Step 1: Add market tools**

```typescript
export const listMarkets = tool(
  async ({ category, status, series, closing_before, closing_after, limit }) => {
    const params = new URLSearchParams();
    if (category) params.set("category", category);
    if (status) params.set("status", status);
    if (series) params.set("series", series);
    if (closing_before) params.set("closing_before", closing_before);
    if (closing_after) params.set("closing_after", closing_after);
    if (limit) params.set("limit", String(limit));

    const path = `/markets${params.toString() ? "?" + params : ""}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "list_markets",
    description: "List markets from secmaster with filters. Returns markets with event metadata.",
    schema: z.object({
      category: z.string().optional().describe("Filter by category (e.g., 'Economics')"),
      status: z.string().optional().describe("Filter by status: open, closed, settled"),
      series: z.string().optional().describe("Filter by series ticker (e.g., 'INXD')"),
      closing_before: z.string().optional().describe("ISO timestamp - markets closing before this time"),
      closing_after: z.string().optional().describe("ISO timestamp - markets closing after this time"),
      limit: z.number().optional().describe("Max results (default 100)"),
    }),
  }
);

export const getMarket = tool(
  async ({ ticker }) => {
    const path = `/markets/${encodeURIComponent(ticker)}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "get_market",
    description: "Get details for a specific market by ticker.",
    schema: z.object({
      ticker: z.string().describe("Market ticker (e.g., 'INXD-25JAN01-B4550')"),
    }),
  }
);

export const getFees = tool(
  async ({ tier }) => {
    const params = new URLSearchParams();
    if (tier) params.set("tier", tier);
    const path = `/fees${params.toString() ? "?" + params : ""}`;
    return JSON.stringify(await apiRequest(path));
  },
  {
    name: "get_fees",
    description: "Get fee schedule (maker/taker fees) for a tier.",
    schema: z.object({
      tier: z.string().optional().describe("Fee tier (default: 'default')"),
    }),
  }
);
```

**Step 2: Add to allTools**

```typescript
export const secmasterTools = [listMarkets, getMarket, getFees];
export const allTools = [...calendarTools, ...dataTools, ...secmasterTools, runBacktest, deploySignal];
```

**Step 3: Verify type check**

Run: `cd /workspaces/ssmd && make agent-check`
Expected: Check passes

**Step 4: Commit**

```bash
git add ssmd-agent/src/agent/tools.ts
git commit -m "feat(agent): add secmaster tools (list_markets, get_market, get_fees)"
```

---

## Task 8: Final Validation

**Step 1: Run all tests**

Run: `cd /workspaces/ssmd && make all-test`
Expected: All tests pass

**Step 2: Run lint**

Run: `cd /workspaces/ssmd && make all-lint`
Expected: No errors

**Step 3: Build all**

Run: `cd /workspaces/ssmd && make all-build`
Expected: Build succeeds

**Step 4: Final commit and tag**

```bash
git add -A
git commit -m "feat: complete secmaster Phase 1 MVP"
git tag -a v0.3.0 -m "feat: secmaster integration"
git push origin main --tags
```

---

## Summary

After completing all tasks:

1. **Types** - Event, Market, Fee structs in `internal/types/secmaster.go`
2. **Database** - PostgreSQL schema in `migrations/001_secmaster.sql`
3. **Store** - CRUD operations in `internal/secmaster/store.go`
4. **Kalshi Client** - REST API client in `internal/secmaster/kalshi.go`
5. **CLI** - `ssmd secmaster sync` command
6. **API** - `/markets`, `/markets/{ticker}`, `/fees` endpoints
7. **Agent Tools** - `list_markets`, `get_market`, `get_fees`

The agent can now:
- Discover markets by category, status, series, close time
- Get market details including event metadata
- Query fee schedules for cost estimation
