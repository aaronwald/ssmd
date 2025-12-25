package data

import (
	"bufio"
	"bytes"
	"compress/gzip"
	"encoding/json"
	"os"
	"strings"
)

// parseRecords extracts records from a gzip reader with filtering
func parseRecords(gr *gzip.Reader, tickerFilter string, typeFilter string, limit int) ([]map[string]interface{}, error) {
	var records []map[string]interface{}
	scanner := bufio.NewScanner(gr)

	for scanner.Scan() {
		if limit > 0 && len(records) >= limit {
			break
		}

		line := scanner.Text()
		if line == "" {
			continue
		}

		var record map[string]interface{}
		if err := json.Unmarshal([]byte(line), &record); err != nil {
			continue // Skip malformed lines
		}

		// Apply ticker filter
		if tickerFilter != "" {
			ticker, ok := record["ticker"].(string)
			if !ok {
				// Check nested msg.market_ticker
				if msg, ok := record["msg"].(map[string]interface{}); ok {
					ticker, _ = msg["market_ticker"].(string)
				}
			}
			if !strings.EqualFold(ticker, tickerFilter) {
				continue
			}
		}

		// Apply type filter
		if typeFilter != "" {
			msgType, _ := record["type"].(string)
			if !strings.EqualFold(msgType, typeFilter) {
				continue
			}
		}

		records = append(records, record)
	}

	return records, scanner.Err()
}

// ReadJSONLGZ reads records from a gzipped JSONL file with optional filters
func ReadJSONLGZ(path string, tickerFilter string, typeFilter string, limit int) ([]map[string]interface{}, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	gr, err := gzip.NewReader(f)
	if err != nil {
		return nil, err
	}
	defer gr.Close()

	return parseRecords(gr, tickerFilter, typeFilter, limit)
}

// ReadJSONLGZFromBytes reads records from gzipped JSONL bytes
func ReadJSONLGZFromBytes(data []byte, tickerFilter string, typeFilter string, limit int) ([]map[string]interface{}, error) {
	gr, err := gzip.NewReader(bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	defer gr.Close()

	return parseRecords(gr, tickerFilter, typeFilter, limit)
}
