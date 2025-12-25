// internal/api/server_test.go
package api

import (
	"bytes"
	"compress/gzip"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/aaronwald/ssmd/internal/types"
)

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
				{"type": "trade", "ticker": "INXD", "price": 0.50},
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

func TestSampleEndpointWithTypeFilter(t *testing.T) {
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
				{"type": "trade", "ticker": "INXD", "price": 0.50},
				{"type": "orderbook", "ticker": "INXD", "yes_ask": 0.55},
			}),
		},
	}

	server := NewServer(storage, "test-key")

	req := httptest.NewRequest("GET", "/datasets/kalshi/2025-12-25/sample?type=trade", nil)
	req.Header.Set("X-API-Key", "test-key")
	rec := httptest.NewRecorder()

	server.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Errorf("expected 200, got %d", rec.Code)
	}

	var records []map[string]interface{}
	if err := json.NewDecoder(rec.Body).Decode(&records); err != nil {
		t.Fatalf("decoding response: %v", err)
	}

	if len(records) != 1 {
		t.Errorf("expected 1 trade record, got %d", len(records))
	}

	if records[0]["type"] != "trade" {
		t.Errorf("expected type trade, got %v", records[0]["type"])
	}
}

func TestSampleEndpointNotFound(t *testing.T) {
	server := NewServer(&mockStorage{}, "test-key")

	req := httptest.NewRequest("GET", "/datasets/unknown/2025-12-25/sample", nil)
	req.Header.Set("X-API-Key", "test-key")
	rec := httptest.NewRecorder()

	server.ServeHTTP(rec, req)

	if rec.Code != http.StatusNotFound {
		t.Errorf("expected 404, got %d", rec.Code)
	}
}

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

	if len(schema.Fields) == 0 {
		t.Error("expected fields to be populated")
	}
}

func TestSchemaEndpointUnknownFeed(t *testing.T) {
	server := NewServer(&mockStorage{}, "test-key")

	req := httptest.NewRequest("GET", "/schema/unknown/orderbook", nil)
	req.Header.Set("X-API-Key", "test-key")
	rec := httptest.NewRecorder()

	server.ServeHTTP(rec, req)

	if rec.Code != http.StatusNotFound {
		t.Errorf("expected 404, got %d", rec.Code)
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

	// Check orderbook builder exists
	found := false
	for _, b := range builders {
		if b.ID == "orderbook" {
			found = true
			break
		}
	}
	if !found {
		t.Error("expected orderbook builder")
	}
}
