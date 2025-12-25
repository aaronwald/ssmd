package cmd

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDataListCommand(t *testing.T) {
	cmd := DataCommand()

	// Verify subcommands exist
	subcommands := cmd.Commands()
	names := make([]string, len(subcommands))
	for i, c := range subcommands {
		names[i] = c.Name()
	}

	expected := []string{"list", "sample", "schema", "builders"}
	for _, exp := range expected {
		found := false
		for _, name := range names {
			if name == exp {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("expected subcommand %q not found", exp)
		}
	}
}

func TestDataListOutput(t *testing.T) {
	// Save original values
	origPath := dataPath
	origOutput := dataOutput
	origFeed := dataFeed
	origFrom := dataFrom
	origTo := dataTo

	// Restore on cleanup
	t.Cleanup(func() {
		dataPath = origPath
		dataOutput = origOutput
		dataFeed = origFeed
		dataFrom = origFrom
		dataTo = origTo
	})

	// Create temp data directory
	tmp := t.TempDir()

	os.MkdirAll(filepath.Join(tmp, "kalshi", "2025-12-25"), 0755)
	manifest := `{"feed":"kalshi","date":"2025-12-25","format":"jsonl","files":[{"name":"1200.jsonl.gz","records":1500,"bytes":50000,"start":"2025-12-25T12:00:00Z","end":"2025-12-25T12:05:00Z","nats_start_seq":1,"nats_end_seq":1500}],"tickers":["INXD","KXBTC"],"message_types":["trade","ticker"],"has_gaps":false}`
	os.WriteFile(filepath.Join(tmp, "kalshi", "2025-12-25", "manifest.json"), []byte(manifest), 0644)

	// Set path flag
	dataPath = tmp
	dataOutput = "json"
	dataFeed = ""
	dataFrom = ""
	dataTo = ""

	var buf bytes.Buffer
	dataListCmd.SetOut(&buf)

	err := runDataList(dataListCmd, []string{})
	if err != nil {
		t.Fatalf("runDataList failed: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "kalshi") {
		t.Error("expected output to contain kalshi")
	}
	if !strings.Contains(output, "2025-12-25") {
		t.Error("expected output to contain date")
	}
	if !strings.Contains(output, "1500") {
		t.Error("expected output to contain record count")
	}
}

func TestDataSchemaOutput(t *testing.T) {
	origOutput := dataOutput
	t.Cleanup(func() { dataOutput = origOutput })

	dataOutput = "json"

	var buf bytes.Buffer
	dataSchemaCmd.SetOut(&buf)

	err := runDataSchema(dataSchemaCmd, []string{"kalshi", "orderbook"})
	if err != nil {
		t.Fatalf("runDataSchema failed: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "yes_bid") {
		t.Error("expected schema to contain yes_bid field")
	}
	if !strings.Contains(output, "spread") {
		t.Error("expected schema to contain spread derived field")
	}
}

func TestDataBuildersOutput(t *testing.T) {
	origOutput := dataOutput
	t.Cleanup(func() { dataOutput = origOutput })

	dataOutput = "json"

	var buf bytes.Buffer
	dataBuildersCmd.SetOut(&buf)

	err := runDataBuilders(dataBuildersCmd, []string{})
	if err != nil {
		t.Fatalf("runDataBuilders failed: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "orderbook") {
		t.Error("expected builders to contain orderbook")
	}
	if !strings.Contains(output, "priceHistory") {
		t.Error("expected builders to contain priceHistory")
	}
}
