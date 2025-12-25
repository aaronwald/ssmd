package data

import (
	"compress/gzip"
	"os"
	"path/filepath"
	"testing"
)

func TestReadJSONLGZ(t *testing.T) {
	tmp := t.TempDir()

	// Create gzipped JSONL file
	filePath := filepath.Join(tmp, "test.jsonl.gz")
	f, _ := os.Create(filePath)
	gw := gzip.NewWriter(f)
	gw.Write([]byte(`{"type":"trade","ticker":"INXD","price":0.55}` + "\n"))
	gw.Write([]byte(`{"type":"trade","ticker":"KXBTC","price":0.42}` + "\n"))
	gw.Write([]byte(`{"type":"ticker","ticker":"INXD","yes_bid":0.50}` + "\n"))
	gw.Close()
	f.Close()

	records, err := ReadJSONLGZ(filePath, "", "", 10)
	if err != nil {
		t.Fatalf("ReadJSONLGZ failed: %v", err)
	}

	if len(records) != 3 {
		t.Errorf("expected 3 records, got %d", len(records))
	}
}

func TestReadJSONLGZWithFilter(t *testing.T) {
	tmp := t.TempDir()

	filePath := filepath.Join(tmp, "test.jsonl.gz")
	f, _ := os.Create(filePath)
	gw := gzip.NewWriter(f)
	gw.Write([]byte(`{"type":"trade","ticker":"INXD","price":0.55}` + "\n"))
	gw.Write([]byte(`{"type":"trade","ticker":"KXBTC","price":0.42}` + "\n"))
	gw.Write([]byte(`{"type":"ticker","ticker":"INXD","yes_bid":0.50}` + "\n"))
	gw.Close()
	f.Close()

	// Filter by ticker
	records, err := ReadJSONLGZ(filePath, "INXD", "", 10)
	if err != nil {
		t.Fatalf("ReadJSONLGZ failed: %v", err)
	}

	if len(records) != 2 {
		t.Errorf("expected 2 records for INXD, got %d", len(records))
	}

	// Filter by type
	records, err = ReadJSONLGZ(filePath, "", "trade", 10)
	if err != nil {
		t.Fatalf("ReadJSONLGZ failed: %v", err)
	}

	if len(records) != 2 {
		t.Errorf("expected 2 trade records, got %d", len(records))
	}
}

func TestReadJSONLGZLimit(t *testing.T) {
	tmp := t.TempDir()

	filePath := filepath.Join(tmp, "test.jsonl.gz")
	f, _ := os.Create(filePath)
	gw := gzip.NewWriter(f)
	for i := 0; i < 100; i++ {
		gw.Write([]byte(`{"type":"trade","ticker":"INXD"}` + "\n"))
	}
	gw.Close()
	f.Close()

	records, err := ReadJSONLGZ(filePath, "", "", 5)
	if err != nil {
		t.Fatalf("ReadJSONLGZ failed: %v", err)
	}

	if len(records) != 5 {
		t.Errorf("expected 5 records (limited), got %d", len(records))
	}
}
