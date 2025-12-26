// internal/api/handlers.go
package api

import (
	"encoding/json"
	"net/http"
	"strconv"
	"time"

	"github.com/aaronwald/ssmd/internal/data"
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
	if err != nil || manifest == nil {
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

	if allRecords == nil {
		allRecords = []map[string]interface{}{}
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(allRecords)
}

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

// Known schemas for each feed
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

func (s *Server) handleTickers(w http.ResponseWriter, r *http.Request) {
	feed := r.PathValue("feed")
	date := r.PathValue("date")

	manifest, err := s.storage.GetManifest(feed, date)
	if err != nil || manifest == nil {
		http.Error(w, `{"error":"dataset not found"}`, http.StatusNotFound)
		return
	}

	tickers := manifest.Tickers
	if tickers == nil {
		tickers = []string{}
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(tickers)
}
