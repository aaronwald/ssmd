// internal/cmd/secmaster.go
package cmd

import (
	"context"
	"database/sql"
	"fmt"
	"os"
	"time"

	"github.com/aaronwald/ssmd/internal/secmaster"
	"github.com/spf13/cobra"

	_ "github.com/lib/pq"
)

var secmasterCmd = &cobra.Command{
	Use:   "secmaster",
	Short: "Manage security master data",
}

var syncCmd = &cobra.Command{
	Use:   "sync",
	Short: "Sync events and markets from Kalshi",
	RunE:  runSync,
}

var (
	syncIncremental bool
)

func init() {
	syncCmd.Flags().BoolVar(&syncIncremental, "incremental", false, "Incremental sync (1-day window)")
	secmasterCmd.AddCommand(syncCmd)
}

// SecmasterCommand returns the secmaster command
func SecmasterCommand() *cobra.Command {
	return secmasterCmd
}

func runSync(cmd *cobra.Command, args []string) error {
	dbURL := os.Getenv("DATABASE_URL")
	if dbURL == "" {
		return fmt.Errorf("DATABASE_URL required")
	}

	db, err := sql.Open("postgres", dbURL)
	if err != nil {
		return fmt.Errorf("connect to database: %w", err)
	}
	defer db.Close()

	store := secmaster.NewStore(db)
	client := secmaster.NewKalshiClient()

	// Calculate min_close_ts (30 days for full, 1 day for incremental)
	windowDays := 30
	if syncIncremental {
		windowDays = 1
	}
	minCloseTS := time.Now().Add(-time.Duration(windowDays) * 24 * time.Hour).Unix()

	ctx := context.Background()

	// Fetch and upsert events
	fmt.Printf("Fetching events (window: %d days)...\n", windowDays)
	events, err := client.FetchAllEvents(minCloseTS)
	if err != nil {
		return fmt.Errorf("fetch events: %w", err)
	}
	fmt.Printf("Fetched %d events\n", len(events))

	for _, e := range events {
		if err := store.UpsertEvent(ctx, &e); err != nil {
			return fmt.Errorf("upsert event %s: %w", e.EventTicker, err)
		}
	}

	// Fetch and upsert markets
	fmt.Printf("Fetching markets...\n")
	markets, err := client.FetchAllMarkets(minCloseTS)
	if err != nil {
		return fmt.Errorf("fetch markets: %w", err)
	}
	fmt.Printf("Fetched %d markets\n", len(markets))

	for _, m := range markets {
		if err := store.UpsertMarket(ctx, &m); err != nil {
			return fmt.Errorf("upsert market %s: %w", m.Ticker, err)
		}
	}

	fmt.Printf("Sync complete: %d events, %d markets\n", len(events), len(markets))
	return nil
}
