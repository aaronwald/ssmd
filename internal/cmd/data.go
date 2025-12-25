package cmd

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/aaronwald/ssmd/internal/data"
	"github.com/spf13/cobra"
)

var dataCmd = &cobra.Command{
	Use:   "data",
	Short: "Query archived market data",
	Long:  `List, sample, and explore archived market data from local storage or GCS.`,
}

var dataListCmd = &cobra.Command{
	Use:   "list",
	Short: "List available datasets",
	RunE:  runDataList,
}

var dataSampleCmd = &cobra.Command{
	Use:   "sample <feed> <date>",
	Short: "Sample records from a dataset",
	Args:  cobra.ExactArgs(2),
	RunE:  runDataSample,
}

var dataSchemaCmd = &cobra.Command{
	Use:   "schema <feed> <message_type>",
	Short: "Show schema for a message type",
	Args:  cobra.ExactArgs(2),
	RunE:  runDataSchema,
}

var dataBuildersCmd = &cobra.Command{
	Use:   "builders",
	Short: "List available state builders",
	RunE:  runDataBuilders,
}

// Flags
var (
	dataFeed       string
	dataFrom       string
	dataTo         string
	dataTicker     string
	dataLimit      int
	dataType       string
	dataOutput     string
	dataPath       string
)

// isJSONOutput returns true if JSON output format is requested
func isJSONOutput() bool {
	return dataOutput == "json"
}

func init() {
	// List flags
	dataListCmd.Flags().StringVar(&dataFeed, "feed", "", "Filter by feed name")
	dataListCmd.Flags().StringVar(&dataFrom, "from", "", "Start date (YYYY-MM-DD)")
	dataListCmd.Flags().StringVar(&dataTo, "to", "", "End date (YYYY-MM-DD)")
	dataListCmd.Flags().StringVar(&dataOutput, "output", "", "Output format (json)")
	dataListCmd.Flags().StringVar(&dataPath, "path", "", "Data path (default: $SSMD_DATA_PATH or gs://ssmd-archive)")

	// Sample flags
	dataSampleCmd.Flags().StringVar(&dataTicker, "ticker", "", "Filter by ticker")
	dataSampleCmd.Flags().IntVar(&dataLimit, "limit", 10, "Max records to return")
	dataSampleCmd.Flags().StringVar(&dataType, "type", "", "Message type (trade, ticker, orderbook)")
	dataSampleCmd.Flags().StringVar(&dataOutput, "output", "", "Output format (json)")
	dataSampleCmd.Flags().StringVar(&dataPath, "path", "", "Data path")

	// Schema flags
	dataSchemaCmd.Flags().StringVar(&dataOutput, "output", "", "Output format (json)")

	// Builders flags
	dataBuildersCmd.Flags().StringVar(&dataOutput, "output", "", "Output format (json)")

	// Add subcommands
	dataCmd.AddCommand(dataListCmd)
	dataCmd.AddCommand(dataSampleCmd)
	dataCmd.AddCommand(dataSchemaCmd)
	dataCmd.AddCommand(dataBuildersCmd)
}

// DataCommand returns the data command for registration
func DataCommand() *cobra.Command {
	return dataCmd
}

// DatasetInfo represents a dataset for list output
type DatasetInfo struct {
	Feed    string  `json:"feed"`
	Date    string  `json:"date"`
	Records uint64  `json:"records"`
	Tickers int     `json:"tickers"`
	SizeMB  float64 `json:"size_mb"`
	HasGaps bool    `json:"has_gaps"`
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

// State builders
var stateBuilders = []BuilderInfo{
	{
		ID:          "orderbook",
		Description: "Maintains bid/ask levels from orderbook updates",
		Derived:     []string{"spread", "bestBid", "bestAsk", "bidDepth", "askDepth", "midpoint"},
	},
	{
		ID:          "priceHistory",
		Description: "Rolling window of price history",
		Derived:     []string{"last", "vwap", "returns", "high", "low", "volatility"},
	},
	{
		ID:          "volumeProfile",
		Description: "Buy/sell volume tracking",
		Derived:     []string{"buyVolume", "sellVolume", "totalVolume", "ratio", "average"},
	},
}

// Known schemas for Kalshi
var knownSchemas = map[string]map[string]SchemaInfo{
	"kalshi": {
		"trade": {
			Type: "trade",
			Fields: map[string]string{
				"ticker":     "string",
				"price":      "number",
				"count":      "number",
				"side":       "string",
				"ts":         "number",
				"taker_side": "string",
			},
			Derived: []string{},
		},
		"ticker": {
			Type: "ticker",
			Fields: map[string]string{
				"ticker":        "string",
				"yes_bid":       "number",
				"yes_ask":       "number",
				"no_bid":        "number",
				"no_ask":        "number",
				"last_price":    "number",
				"volume":        "number",
				"open_interest": "number",
				"ts":            "number",
			},
			Derived: []string{"spread", "midpoint"},
		},
		"orderbook": {
			Type: "orderbook",
			Fields: map[string]string{
				"ticker":  "string",
				"yes_bid": "number",
				"yes_ask": "number",
				"no_bid":  "number",
				"no_ask":  "number",
				"ts":      "number",
			},
			Derived: []string{"spread", "midpoint", "imbalance"},
		},
	},
}

// Placeholder implementations
func runDataList(cmd *cobra.Command, args []string) error {
	// Determine data path
	path := dataPath
	if path == "" {
		path = os.Getenv("SSMD_DATA_PATH")
	}
	if path == "" {
		return fmt.Errorf("data path not specified (use --path or SSMD_DATA_PATH)")
	}

	storage, err := data.NewStorage(path)
	if err != nil {
		return fmt.Errorf("creating storage: %w", err)
	}

	// List feeds
	feeds, err := storage.ListFeeds()
	if err != nil {
		return fmt.Errorf("listing feeds: %w", err)
	}

	// Filter by feed if specified
	if dataFeed != "" {
		filtered := []string{}
		for _, f := range feeds {
			if f == dataFeed {
				filtered = append(filtered, f)
			}
		}
		feeds = filtered
	}

	// Parse date range
	var fromDate, toDate time.Time
	if dataFrom != "" {
		fromDate, err = time.Parse("2006-01-02", dataFrom)
		if err != nil {
			return fmt.Errorf("invalid from date: %w", err)
		}
	}
	if dataTo != "" {
		toDate, err = time.Parse("2006-01-02", dataTo)
		if err != nil {
			return fmt.Errorf("invalid to date: %w", err)
		}
	}

	// Collect dataset info
	var datasets []DatasetInfo
	for _, feed := range feeds {
		dates, err := storage.ListDates(feed)
		if err != nil {
			continue // Skip feeds with errors
		}

		for _, date := range dates {
			// Filter by date range
			if dataFrom != "" || dataTo != "" {
				d, err := time.Parse("2006-01-02", date)
				if err != nil {
					continue
				}
				if dataFrom != "" && d.Before(fromDate) {
					continue
				}
				if dataTo != "" && d.After(toDate) {
					continue
				}
			}

			manifest, err := storage.GetManifest(feed, date)
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

	// Output
	if isJSONOutput() {
		enc := json.NewEncoder(cmd.OutOrStdout())
		enc.SetIndent("", "  ")
		return enc.Encode(datasets)
	}

	// Table output
	fmt.Fprintf(cmd.OutOrStdout(), "%-12s %-12s %10s %8s %10s %s\n",
		"FEED", "DATE", "RECORDS", "TICKERS", "SIZE", "GAPS")
	for _, d := range datasets {
		gaps := ""
		if d.HasGaps {
			gaps = "YES"
		}
		fmt.Fprintf(cmd.OutOrStdout(), "%-12s %-12s %10d %8d %9.1fMB %s\n",
			d.Feed, d.Date, d.Records, d.Tickers, d.SizeMB, gaps)
	}

	return nil
}

func runDataSample(cmd *cobra.Command, args []string) error {
	feed := args[0]
	date := args[1]

	path := dataPath
	if path == "" {
		path = os.Getenv("SSMD_DATA_PATH")
	}
	if path == "" {
		return fmt.Errorf("data path not specified (use --path or SSMD_DATA_PATH)")
	}

	storage, err := data.NewStorage(path)
	if err != nil {
		return fmt.Errorf("creating storage: %w", err)
	}

	// Get manifest to find files
	manifest, err := storage.GetManifest(feed, date)
	if err != nil {
		return fmt.Errorf("getting manifest: %w", err)
	}

	if len(manifest.Files) == 0 {
		return fmt.Errorf("no files in manifest for %s/%s", feed, date)
	}

	// Read from files up to limit
	var allRecords []map[string]interface{}
	remaining := dataLimit

	for _, file := range manifest.Files {
		if remaining <= 0 {
			break
		}

		fileData, err := storage.ReadFile(feed, date, file.Name)
		if err != nil {
			continue
		}

		records, err := data.ReadJSONLGZFromBytes(fileData, dataTicker, dataType, remaining)
		if err != nil {
			continue
		}

		allRecords = append(allRecords, records...)
		remaining -= len(records)
	}

	// Output
	if isJSONOutput() {
		enc := json.NewEncoder(cmd.OutOrStdout())
		enc.SetIndent("", "  ")
		return enc.Encode(allRecords)
	}

	// Pretty print each record
	for _, r := range allRecords {
		b, err := json.MarshalIndent(r, "", "  ")
		if err != nil {
			return fmt.Errorf("marshaling record: %w", err)
		}
		fmt.Fprintln(cmd.OutOrStdout(), string(b))
	}

	return nil
}

func runDataSchema(cmd *cobra.Command, args []string) error {
	feed := args[0]
	msgType := args[1]

	feedSchemas, ok := knownSchemas[feed]
	if !ok {
		return fmt.Errorf("unknown feed: %s", feed)
	}

	schema, ok := feedSchemas[msgType]
	if !ok {
		return fmt.Errorf("unknown message type %s for feed %s", msgType, feed)
	}

	if isJSONOutput() {
		enc := json.NewEncoder(cmd.OutOrStdout())
		enc.SetIndent("", "  ")
		return enc.Encode(schema)
	}

	// Table output
	fmt.Fprintf(cmd.OutOrStdout(), "Schema: %s.%s\n\n", feed, msgType)
	fmt.Fprintf(cmd.OutOrStdout(), "Fields:\n")
	for name, typ := range schema.Fields {
		fmt.Fprintf(cmd.OutOrStdout(), "  %-20s %s\n", name, typ)
	}
	if len(schema.Derived) > 0 {
		fmt.Fprintf(cmd.OutOrStdout(), "\nDerived:\n")
		for _, d := range schema.Derived {
			fmt.Fprintf(cmd.OutOrStdout(), "  %s\n", d)
		}
	}

	return nil
}

func runDataBuilders(cmd *cobra.Command, args []string) error {
	if isJSONOutput() {
		enc := json.NewEncoder(cmd.OutOrStdout())
		enc.SetIndent("", "  ")
		return enc.Encode(stateBuilders)
	}

	// Table output
	fmt.Fprintf(cmd.OutOrStdout(), "%-15s %-45s %s\n", "ID", "DESCRIPTION", "DERIVED FIELDS")
	for _, b := range stateBuilders {
		derived := strings.Join(b.Derived, ", ")
		fmt.Fprintf(cmd.OutOrStdout(), "%-15s %-45s %s\n", b.ID, b.Description, derived)
	}

	return nil
}
