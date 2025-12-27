// internal/cmd/secmaster.go
package cmd

import (
	"context"
	"database/sql"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/aaronwald/ssmd/internal/secmaster"
	"github.com/aaronwald/ssmd/internal/types"
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

var listCmd = &cobra.Command{
	Use:   "list",
	Short: "List markets from secmaster",
	RunE:  runList,
}

var showCmd = &cobra.Command{
	Use:   "show <ticker>",
	Short: "Show details for a market",
	Args:  cobra.ExactArgs(1),
	RunE:  runShow,
}

var statsCmd = &cobra.Command{
	Use:   "stats",
	Short: "Show secmaster summary statistics",
	RunE:  runStats,
}

var (
	syncIncremental bool
	listCategory    string
	listStatus      string
	listSeries      string
	listLimit       int
)

func init() {
	syncCmd.Flags().BoolVar(&syncIncremental, "incremental", false, "Incremental sync (1-day window)")

	listCmd.Flags().StringVar(&listCategory, "category", "", "Filter by category (e.g., Economics, Politics)")
	listCmd.Flags().StringVar(&listStatus, "status", "", "Filter by status (open, closed, settled)")
	listCmd.Flags().StringVar(&listSeries, "series", "", "Filter by series ticker")
	listCmd.Flags().IntVar(&listLimit, "limit", 20, "Maximum number of results")

	secmasterCmd.AddCommand(syncCmd)
	secmasterCmd.AddCommand(listCmd)
	secmasterCmd.AddCommand(showCmd)
	secmasterCmd.AddCommand(statsCmd)
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

	// Stream events - upsert each page as it arrives
	fmt.Printf("Syncing events (window: %d days)...\n", windowDays)
	eventCount, eventCursor, err := client.StreamEvents(minCloseTS, "", func(events []types.Event, cursor string) error {
		if err := store.UpsertEventBatch(ctx, events); err != nil {
			return err
		}
		fmt.Printf("  Events: +%d (cursor: %s)\n", len(events), truncateCursor(cursor))
		return nil
	})
	if err != nil {
		if eventCursor != "" {
			fmt.Printf("Partial sync failed at cursor: %s\n", eventCursor)
		}
		return fmt.Errorf("sync events: %w", err)
	}

	// Stream markets - upsert each page as it arrives
	fmt.Printf("Syncing markets...\n")
	var totalSkipped int
	marketCount, marketCursor, err := client.StreamMarkets(minCloseTS, "", func(markets []types.Market, cursor string) error {
		skipped, err := store.UpsertMarketBatch(ctx, markets)
		if err != nil {
			return err
		}
		totalSkipped += skipped
		if skipped > 0 {
			fmt.Printf("  Markets: +%d (skipped: %d, cursor: %s)\n", len(markets)-skipped, skipped, truncateCursor(cursor))
		} else {
			fmt.Printf("  Markets: +%d (cursor: %s)\n", len(markets), truncateCursor(cursor))
		}
		return nil
	})
	if err != nil {
		if marketCursor != "" {
			fmt.Printf("Partial sync failed at cursor: %s\n", marketCursor)
		}
		return fmt.Errorf("sync markets: %w", err)
	}

	if totalSkipped > 0 {
		fmt.Printf("Skipped %d markets with missing parent events\n", totalSkipped)
	}
	fmt.Printf("Sync complete: %d events, %d markets synced\n", eventCount, marketCount-totalSkipped)
	return nil
}

func truncateCursor(cursor string) string {
	if cursor == "" {
		return "done"
	}
	if len(cursor) > 16 {
		return cursor[:16] + "..."
	}
	return cursor
}

func runList(cmd *cobra.Command, args []string) error {
	db, err := openDB()
	if err != nil {
		return err
	}
	defer db.Close()

	store := secmaster.NewStore(db)
	opts := secmaster.MarketListOptions{
		Category: listCategory,
		Status:   listStatus,
		Series:   listSeries,
		Limit:    listLimit,
	}

	markets, err := store.ListMarkets(context.Background(), opts)
	if err != nil {
		return fmt.Errorf("list markets: %w", err)
	}

	if len(markets) == 0 {
		fmt.Println("No markets found")
		return nil
	}

	// Print header
	fmt.Printf("%-35s %-8s %-15s %s\n", "TICKER", "STATUS", "CATEGORY", "TITLE")
	fmt.Println(strings.Repeat("-", 100))

	for _, m := range markets {
		title := m.Title
		if len(title) > 40 {
			title = title[:37] + "..."
		}
		fmt.Printf("%-35s %-8s %-15s %s\n", m.Ticker, m.Status, m.Category, title)
	}

	fmt.Printf("\nShowing %d markets", len(markets))
	if listLimit > 0 && len(markets) >= listLimit {
		fmt.Printf(" (limit %d)", listLimit)
	}
	fmt.Println()

	return nil
}

func runShow(cmd *cobra.Command, args []string) error {
	ticker := args[0]

	db, err := openDB()
	if err != nil {
		return err
	}
	defer db.Close()

	store := secmaster.NewStore(db)
	market, err := store.GetMarket(context.Background(), ticker)
	if err != nil {
		return fmt.Errorf("get market: %w", err)
	}

	if market == nil {
		return fmt.Errorf("market not found: %s", ticker)
	}

	fmt.Printf("Ticker:        %s\n", market.Ticker)
	fmt.Printf("Title:         %s\n", market.Title)
	fmt.Printf("Status:        %s\n", market.Status)
	fmt.Printf("Event:         %s\n", market.EventTicker)
	fmt.Printf("Event Title:   %s\n", market.EventTitle)
	fmt.Printf("Category:      %s\n", market.Category)
	fmt.Printf("Series:        %s\n", market.SeriesTicker)

	if market.CloseTime != nil {
		fmt.Printf("Close Time:    %s\n", market.CloseTime.Format(time.RFC3339))
	}

	fmt.Println()
	fmt.Println("Pricing:")
	if market.YesBid != nil && market.YesAsk != nil {
		fmt.Printf("  Yes:  %d / %d (bid/ask)\n", *market.YesBid, *market.YesAsk)
	}
	if market.NoBid != nil && market.NoAsk != nil {
		fmt.Printf("  No:   %d / %d (bid/ask)\n", *market.NoBid, *market.NoAsk)
	}
	if market.LastPrice != nil {
		fmt.Printf("  Last: %d\n", *market.LastPrice)
	}

	fmt.Println()
	fmt.Println("Volume:")
	if market.Volume != nil {
		fmt.Printf("  Total:    %d\n", *market.Volume)
	}
	if market.Volume24h != nil {
		fmt.Printf("  24h:      %d\n", *market.Volume24h)
	}
	if market.OpenInterest != nil {
		fmt.Printf("  Open Int: %d\n", *market.OpenInterest)
	}

	fmt.Println()
	fmt.Printf("Updated:       %s\n", market.UpdatedAt.Format(time.RFC3339))

	return nil
}

func runStats(cmd *cobra.Command, args []string) error {
	db, err := openDB()
	if err != nil {
		return err
	}
	defer db.Close()

	store := secmaster.NewStore(db)
	stats, err := store.GetStats(context.Background())
	if err != nil {
		return fmt.Errorf("get stats: %w", err)
	}

	fmt.Println("Secmaster Statistics")
	fmt.Println(strings.Repeat("=", 40))
	fmt.Printf("Events:              %d\n", stats.TotalEvents)
	fmt.Printf("Markets:             %d\n", stats.TotalMarkets)
	fmt.Printf("Closing in 24h:      %d\n", stats.MarketsClosingSoon)

	if stats.LastSyncTime != nil {
		fmt.Printf("Last sync:           %s\n", stats.LastSyncTime.Format(time.RFC3339))
	}

	fmt.Println()
	fmt.Println("Markets by Status:")
	for status, count := range stats.MarketsByStatus {
		fmt.Printf("  %-12s %d\n", status, count)
	}

	if len(stats.TopCategories) > 0 {
		fmt.Println()
		fmt.Println("Top Categories:")
		for _, cc := range stats.TopCategories {
			fmt.Printf("  %-20s %d\n", cc.Category, cc.Count)
		}
	}

	return nil
}

func openDB() (*sql.DB, error) {
	dbURL := os.Getenv("DATABASE_URL")
	if dbURL == "" {
		return nil, fmt.Errorf("DATABASE_URL required")
	}
	return sql.Open("postgres", dbURL)
}
