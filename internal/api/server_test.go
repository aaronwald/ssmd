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
	fileData  map[string][]byte
}

func (m *mockStorage) ListFeeds() ([]string, error) {
	return m.feeds, nil
}

func (m *mockStorage) ListDates(feed string) ([]string, error) {
	return m.dates[feed], nil
}

func (m *mockStorage) GetManifest(feed, date string) (*types.Manifest, error) {
	if m.manifests == nil {
		return nil, nil
	}
	if m.manifests[feed] == nil {
		return nil, nil
	}
	return m.manifests[feed][date], nil
}

func (m *mockStorage) ReadFile(feed, date, filename string) ([]byte, error) {
	if m.fileData == nil {
		return nil, nil
	}
	key := feed + "/" + date + "/" + filename
	return m.fileData[key], nil
}

func TestHealthEndpoint(t *testing.T) {
	server := NewServer(&mockStorage{}, "test-key")

	req := httptest.NewRequest("GET", "/health", nil)
	rec := httptest.NewRecorder()

	server.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Errorf("expected 200, got %d", rec.Code)
	}

	var result map[string]string
	if err := json.NewDecoder(rec.Body).Decode(&result); err != nil {
		t.Fatalf("decoding response: %v", err)
	}

	if result["status"] != "ok" {
		t.Errorf("expected status ok, got %s", result["status"])
	}
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

	if datasets[0].Records != 1000 {
		t.Errorf("expected 1000 records, got %d", datasets[0].Records)
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

func TestDatasetsWithFeedFilter(t *testing.T) {
	storage := &mockStorage{
		feeds: []string{"kalshi", "polymarket"},
		dates: map[string][]string{
			"kalshi":     {"2025-12-25"},
			"polymarket": {"2025-12-25"},
		},
		manifests: map[string]map[string]*types.Manifest{
			"kalshi": {
				"2025-12-25": {
					Feed:    "kalshi",
					Date:    "2025-12-25",
					Tickers: []string{"INXD"},
					Files:   []types.FileEntry{{Records: 100, Bytes: 5000}},
				},
			},
			"polymarket": {
				"2025-12-25": {
					Feed:    "polymarket",
					Date:    "2025-12-25",
					Tickers: []string{"PRES"},
					Files:   []types.FileEntry{{Records: 200, Bytes: 10000}},
				},
			},
		},
	}

	server := NewServer(storage, "test-key")

	req := httptest.NewRequest("GET", "/datasets?feed=kalshi", nil)
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
