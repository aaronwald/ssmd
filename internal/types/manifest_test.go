// internal/types/manifest_test.go
package types

import (
	"encoding/json"
	"testing"
)

func TestManifestUnmarshal(t *testing.T) {
	data := `{
		"feed": "kalshi",
		"date": "2025-12-25",
		"format": "jsonl",
		"rotation_interval": "5m",
		"files": [
			{
				"name": "1738.jsonl.gz",
				"start": "2025-12-25T17:38:00Z",
				"end": "2025-12-25T17:43:00Z",
				"records": 150,
				"bytes": 12345,
				"nats_start_seq": 100,
				"nats_end_seq": 249
			}
		],
		"gaps": [],
		"tickers": ["INXD-25001", "KXBTC-25001"],
		"message_types": ["trade", "ticker"],
		"has_gaps": false
	}`

	var m Manifest
	err := json.Unmarshal([]byte(data), &m)
	if err != nil {
		t.Fatalf("failed to unmarshal: %v", err)
	}

	if m.Feed != "kalshi" {
		t.Errorf("expected feed kalshi, got %s", m.Feed)
	}
	if m.Date != "2025-12-25" {
		t.Errorf("expected date 2025-12-25, got %s", m.Date)
	}
	if len(m.Files) != 1 {
		t.Errorf("expected 1 file, got %d", len(m.Files))
	}
	if m.Files[0].Records != 150 {
		t.Errorf("expected 150 records, got %d", m.Files[0].Records)
	}
	if len(m.Tickers) != 2 {
		t.Errorf("expected 2 tickers, got %d", len(m.Tickers))
	}
}

func TestManifestTotalRecords(t *testing.T) {
	m := Manifest{
		Files: []FileEntry{
			{Records: 100},
			{Records: 200},
			{Records: 50},
		},
	}

	if m.TotalRecords() != 350 {
		t.Errorf("expected 350 total records, got %d", m.TotalRecords())
	}
}
