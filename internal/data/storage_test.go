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

func TestLocalStoragePathTraversal(t *testing.T) {
	tmp := t.TempDir()
	storage := NewLocalStorage(tmp)

	testCases := []struct {
		name     string
		feed     string
		date     string
		filename string
	}{
		{"simple parent traversal", "../etc", "passwd", ""},
		{"deep traversal", "../../..", "etc", "passwd"},
		{"mixed traversal", "valid/../../../etc", "passwd", ""},
		{"filename traversal", "kalshi", "2025-12-25", "../../../etc/passwd"},
		{"absolute path", "/etc", "passwd", ""},
	}

	for _, tc := range testCases {
		t.Run(tc.name+" via ListDates", func(t *testing.T) {
			_, err := storage.ListDates(tc.feed)
			if err == nil {
				t.Error("expected error for path traversal, got nil")
			}
		})
	}

	for _, tc := range testCases {
		t.Run(tc.name+" via GetManifest", func(t *testing.T) {
			_, err := storage.GetManifest(tc.feed, tc.date)
			if err == nil {
				t.Error("expected error for path traversal, got nil")
			}
		})
	}

	for _, tc := range testCases {
		t.Run(tc.name+" via ReadFile", func(t *testing.T) {
			_, err := storage.ReadFile(tc.feed, tc.date, tc.filename)
			if err == nil {
				t.Error("expected error for path traversal, got nil")
			}
		})
	}
}

func TestLocalStorageValidPaths(t *testing.T) {
	tmp := t.TempDir()

	// Setup test data
	os.MkdirAll(filepath.Join(tmp, "kalshi", "2025-12-25"), 0755)
	testData := []byte("test content")
	os.WriteFile(filepath.Join(tmp, "kalshi", "2025-12-25", "data.jsonl"), testData, 0644)

	storage := NewLocalStorage(tmp)

	// Valid path should succeed
	data, err := storage.ReadFile("kalshi", "2025-12-25", "data.jsonl")
	if err != nil {
		t.Errorf("valid path rejected: %v", err)
	}
	if string(data) != "test content" {
		t.Errorf("unexpected data: got %q, want %q", string(data), "test content")
	}
}

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
