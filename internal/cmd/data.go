package cmd

import (
	"encoding/json"
	"fmt"
	"os"
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
	dataOutputJSON bool
	dataPath       string
)

func init() {
	// List flags
	dataListCmd.Flags().StringVar(&dataFeed, "feed", "", "Filter by feed name")
	dataListCmd.Flags().StringVar(&dataFrom, "from", "", "Start date (YYYY-MM-DD)")
	dataListCmd.Flags().StringVar(&dataTo, "to", "", "End date (YYYY-MM-DD)")
	dataListCmd.Flags().BoolVar(&dataOutputJSON, "output", false, "Output as JSON (use --output json)")
	dataListCmd.Flags().StringVar(&dataPath, "path", "", "Data path (default: $SSMD_DATA_PATH or gs://ssmd-archive)")

	// Sample flags
	dataSampleCmd.Flags().StringVar(&dataTicker, "ticker", "", "Filter by ticker")
	dataSampleCmd.Flags().IntVar(&dataLimit, "limit", 10, "Max records to return")
	dataSampleCmd.Flags().StringVar(&dataType, "type", "", "Message type (trade, ticker, orderbook)")
	dataSampleCmd.Flags().BoolVar(&dataOutputJSON, "output", false, "Output as JSON")
	dataSampleCmd.Flags().StringVar(&dataPath, "path", "", "Data path")

	// Schema flags
	dataSchemaCmd.Flags().BoolVar(&dataOutputJSON, "output", false, "Output as JSON")

	// Builders flags
	dataBuildersCmd.Flags().BoolVar(&dataOutputJSON, "output", false, "Output as JSON")

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
	if dataOutputJSON {
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
	return nil
}

func runDataSchema(cmd *cobra.Command, args []string) error {
	return nil
}

func runDataBuilders(cmd *cobra.Command, args []string) error {
	return nil
}
