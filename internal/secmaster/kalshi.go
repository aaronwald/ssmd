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
	KalshiAPIBase    = "https://api.elections.kalshi.com/trade-api/v2"
	DefaultPageLimit = 200
	RequestTimeout   = 30 * time.Second
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

		reqURL := fmt.Sprintf("%s/events?%s", c.baseURL, params.Encode())
		resp, err := c.httpClient.Get(reqURL)
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

		reqURL := fmt.Sprintf("%s/markets?%s", c.baseURL, params.Encode())
		resp, err := c.httpClient.Get(reqURL)
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
