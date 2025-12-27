// internal/secmaster/kalshi.go
package secmaster

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"net/url"
	"strconv"
	"time"

	"github.com/aaronwald/ssmd/internal/types"
)

const (
	KalshiAPIBase    = "https://api.elections.kalshi.com/trade-api/v2"
	DefaultPageLimit = 200
	MarketsPageLimit = 1000
	RequestTimeout   = 30 * time.Second
	MaxRetries       = 10                 // Increased for aggressive rate limiting
	DefaultRateDelay = 500 * time.Millisecond
	Min429Wait       = 5 * time.Second    // Minimum wait on 429 even if Retry-After is shorter
	Max429Wait       = 120 * time.Second  // Cap Retry-After at 2 min
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

// EventPageHandler processes a batch of events. Return error to abort.
type EventPageHandler func(events []types.Event, cursor string) error

// MarketPageHandler processes a batch of markets. Return error to abort.
type MarketPageHandler func(markets []types.Market, cursor string) error

// doRequest executes HTTP GET with 429 retry handling
func (c *KalshiClient) doRequest(reqURL string) (*http.Response, error) {
	for attempt := 0; attempt < MaxRetries; attempt++ {
		resp, err := c.httpClient.Get(reqURL)
		if err != nil {
			return nil, fmt.Errorf("http request: %w", err)
		}

		if resp.StatusCode == http.StatusOK {
			return resp, nil
		}

		resp.Body.Close() // Close before retry

		if resp.StatusCode == http.StatusTooManyRequests {
			retryAfter := parseRetryAfter(resp.Header.Get("Retry-After"))
			if retryAfter < Min429Wait {
				retryAfter = Min429Wait
			}
			if retryAfter > Max429Wait {
				retryAfter = Max429Wait
			}
			log.Printf("Rate limited, waiting %v (attempt %d/%d)", retryAfter, attempt+1, MaxRetries)
			time.Sleep(retryAfter)
			continue
		}

		return nil, fmt.Errorf("API returned %d", resp.StatusCode)
	}
	return nil, fmt.Errorf("max retries (%d) exceeded", MaxRetries)
}

// parseRetryAfter parses Retry-After header (seconds or HTTP-date)
func parseRetryAfter(header string) time.Duration {
	if header == "" {
		return DefaultRateDelay * 4 // Fallback: 1 second
	}
	// Try parsing as seconds (most common)
	if seconds, err := strconv.Atoi(header); err == nil {
		return time.Duration(seconds) * time.Second
	}
	// Try parsing as HTTP-date (RFC 7231)
	if t, err := http.ParseTime(header); err == nil {
		wait := time.Until(t)
		if wait < 0 {
			return DefaultRateDelay
		}
		return wait
	}
	return DefaultRateDelay * 4
}

// StreamEvents fetches events page-by-page, calling handler for each page.
// Returns total count and last cursor (for resume capability on error).
func (c *KalshiClient) StreamEvents(minCloseTS int64, startCursor string, handler EventPageHandler) (int, string, error) {
	cursor := startCursor
	totalCount := 0

	for {
		params := url.Values{}
		params.Set("limit", fmt.Sprintf("%d", DefaultPageLimit))
		if minCloseTS > 0 {
			params.Set("min_close_ts", fmt.Sprintf("%d", minCloseTS))
		}
		if cursor != "" {
			params.Set("cursor", cursor)
		}

		reqURL := fmt.Sprintf("%s/events?%s", c.baseURL, params.Encode())
		resp, err := c.doRequest(reqURL)
		if err != nil {
			return totalCount, cursor, fmt.Errorf("fetch events page: %w", err)
		}

		var result EventsResponse
		err = json.NewDecoder(resp.Body).Decode(&result)
		resp.Body.Close()
		if err != nil {
			return totalCount, cursor, fmt.Errorf("decode events: %w", err)
		}

		// Convert to domain types
		events := make([]types.Event, 0, len(result.Events))
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
				if t, err := time.Parse(time.RFC3339, *e.StrikeDate); err == nil {
					event.StrikeDate = &t
				}
			}
			events = append(events, event)
		}

		// Call handler with this page
		if err := handler(events, result.Cursor); err != nil {
			return totalCount, cursor, fmt.Errorf("handler error: %w", err)
		}
		totalCount += len(events)

		if result.Cursor == "" {
			break
		}
		cursor = result.Cursor
		time.Sleep(DefaultRateDelay)
	}

	return totalCount, "", nil
}

// StreamMarkets fetches markets page-by-page, calling handler for each page.
// Returns total count and last cursor (for resume capability on error).
func (c *KalshiClient) StreamMarkets(minCloseTS int64, startCursor string, handler MarketPageHandler) (int, string, error) {
	cursor := startCursor
	totalCount := 0

	for {
		params := url.Values{}
		params.Set("limit", fmt.Sprintf("%d", MarketsPageLimit))
		if minCloseTS > 0 {
			params.Set("min_close_ts", fmt.Sprintf("%d", minCloseTS))
		}
		if cursor != "" {
			params.Set("cursor", cursor)
		}

		reqURL := fmt.Sprintf("%s/markets?%s", c.baseURL, params.Encode())
		resp, err := c.doRequest(reqURL)
		if err != nil {
			return totalCount, cursor, fmt.Errorf("fetch markets page: %w", err)
		}

		var result MarketsResponse
		err = json.NewDecoder(resp.Body).Decode(&result)
		resp.Body.Close()
		if err != nil {
			return totalCount, cursor, fmt.Errorf("decode markets: %w", err)
		}

		// Convert to domain types
		markets := make([]types.Market, 0, len(result.Markets))
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
				if t, err := time.Parse(time.RFC3339, *m.CloseTime); err == nil {
					market.CloseTime = &t
				}
			}
			markets = append(markets, market)
		}

		// Call handler with this page
		if err := handler(markets, result.Cursor); err != nil {
			return totalCount, cursor, fmt.Errorf("handler error: %w", err)
		}
		totalCount += len(markets)

		if result.Cursor == "" {
			break
		}
		cursor = result.Cursor
		time.Sleep(DefaultRateDelay)
	}

	return totalCount, "", nil
}

// FetchAllEvents fetches all events with pagination (legacy, accumulates in memory)
func (c *KalshiClient) FetchAllEvents(minCloseTS int64) ([]types.Event, error) {
	var allEvents []types.Event
	_, _, err := c.StreamEvents(minCloseTS, "", func(events []types.Event, cursor string) error {
		allEvents = append(allEvents, events...)
		return nil
	})
	return allEvents, err
}

// FetchAllMarkets fetches all markets with pagination (legacy, accumulates in memory)
func (c *KalshiClient) FetchAllMarkets(minCloseTS int64) ([]types.Market, error) {
	var allMarkets []types.Market
	_, _, err := c.StreamMarkets(minCloseTS, "", func(markets []types.Market, cursor string) error {
		allMarkets = append(allMarkets, markets...)
		return nil
	})
	return allMarkets, err
}
