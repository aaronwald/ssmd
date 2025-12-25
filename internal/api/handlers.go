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
