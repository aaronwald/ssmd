// internal/secmaster/store.go
package secmaster

import (
	"context"
	"database/sql"
	"fmt"
	"strings"
	"time"

	"github.com/aaronwald/ssmd/internal/types"
	"github.com/lib/pq"
)

// Store handles secmaster database operations
//
// TODO: Reconcile direct SQL access vs using the ssmd-data HTTP service from CLI.
// Currently CLI commands (list, show, stats) use Store directly, while the agent
// uses the HTTP API. Consider having CLI call the service for consistency, or
// keep direct access for offline/admin use cases.
type Store struct {
	db *sql.DB
}

// NewStore creates a new secmaster store
func NewStore(db *sql.DB) *Store {
	return &Store{db: db}
}

// UpsertEvent inserts or updates an event
func (s *Store) UpsertEvent(ctx context.Context, e *types.Event) error {
	query := `
		INSERT INTO events (event_ticker, title, category, series_ticker, strike_date, mutually_exclusive, status, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
		ON CONFLICT (event_ticker) DO UPDATE SET
			title = EXCLUDED.title,
			category = EXCLUDED.category,
			series_ticker = EXCLUDED.series_ticker,
			strike_date = EXCLUDED.strike_date,
			mutually_exclusive = EXCLUDED.mutually_exclusive,
			status = EXCLUDED.status,
			updated_at = NOW(),
			deleted_at = NULL
	`
	_, err := s.db.ExecContext(ctx, query,
		e.EventTicker, e.Title, e.Category, e.SeriesTicker,
		e.StrikeDate, e.MutuallyExclusive, e.Status)
	return err
}

// UpsertMarket inserts or updates a market
func (s *Store) UpsertMarket(ctx context.Context, m *types.Market) error {
	query := `
		INSERT INTO markets (ticker, event_ticker, title, status, close_time,
			yes_bid, yes_ask, no_bid, no_ask, last_price, volume, volume_24h, open_interest, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, NOW())
		ON CONFLICT (ticker) DO UPDATE SET
			title = EXCLUDED.title,
			status = EXCLUDED.status,
			close_time = EXCLUDED.close_time,
			yes_bid = EXCLUDED.yes_bid,
			yes_ask = EXCLUDED.yes_ask,
			no_bid = EXCLUDED.no_bid,
			no_ask = EXCLUDED.no_ask,
			last_price = EXCLUDED.last_price,
			volume = EXCLUDED.volume,
			volume_24h = EXCLUDED.volume_24h,
			open_interest = EXCLUDED.open_interest,
			updated_at = NOW(),
			deleted_at = NULL
	`
	_, err := s.db.ExecContext(ctx, query,
		m.Ticker, m.EventTicker, m.Title, m.Status, m.CloseTime,
		m.YesBid, m.YesAsk, m.NoBid, m.NoAsk, m.LastPrice,
		m.Volume, m.Volume24h, m.OpenInterest)
	return err
}

// EventBatchSize is the number of events to insert per bulk query
const EventBatchSize = 500

// UpsertEventBatch upserts multiple events using bulk inserts
func (s *Store) UpsertEventBatch(ctx context.Context, events []types.Event) error {
	if len(events) == 0 {
		return nil
	}

	// Process in batches
	for i := 0; i < len(events); i += EventBatchSize {
		end := i + EventBatchSize
		if end > len(events) {
			end = len(events)
		}
		batch := events[i:end]

		if err := s.bulkUpsertEvents(ctx, batch); err != nil {
			return err
		}
	}

	return nil
}

// bulkUpsertEvents inserts a batch of events using multi-row VALUES
func (s *Store) bulkUpsertEvents(ctx context.Context, events []types.Event) error {
	if len(events) == 0 {
		return nil
	}

	// Build multi-row VALUES clause
	valueStrings := make([]string, 0, len(events))
	valueArgs := make([]interface{}, 0, len(events)*7)

	for i, e := range events {
		base := i * 7
		valueStrings = append(valueStrings, fmt.Sprintf(
			"($%d, $%d, $%d, $%d, $%d, $%d, $%d, NOW())",
			base+1, base+2, base+3, base+4, base+5, base+6, base+7,
		))
		valueArgs = append(valueArgs,
			e.EventTicker, e.Title, e.Category, e.SeriesTicker,
			e.StrikeDate, e.MutuallyExclusive, e.Status,
		)
	}

	query := fmt.Sprintf(`
		INSERT INTO events (event_ticker, title, category, series_ticker, strike_date, mutually_exclusive, status, updated_at)
		VALUES %s
		ON CONFLICT (event_ticker) DO UPDATE SET
			title = EXCLUDED.title,
			category = EXCLUDED.category,
			series_ticker = EXCLUDED.series_ticker,
			strike_date = EXCLUDED.strike_date,
			mutually_exclusive = EXCLUDED.mutually_exclusive,
			status = EXCLUDED.status,
			updated_at = NOW(),
			deleted_at = NULL
	`, strings.Join(valueStrings, ", "))

	result, err := s.db.ExecContext(ctx, query, valueArgs...)
	if err != nil {
		return err
	}
	rows, _ := result.RowsAffected()
	fmt.Printf("    [DB] events batch: %d attempted, %d affected\n", len(events), rows)
	return nil
}

// MarketBatchSize is the number of markets to insert per bulk query
const MarketBatchSize = 500

// UpsertMarketBatch upserts multiple markets using bulk inserts.
// Pre-filters markets by existing events to avoid FK violations.
// Returns count of skipped markets (missing parent events) for logging.
func (s *Store) UpsertMarketBatch(ctx context.Context, markets []types.Market) (int, error) {
	if len(markets) == 0 {
		return 0, nil
	}

	// Collect unique event tickers from this batch
	eventTickerSet := make(map[string]struct{})
	for _, m := range markets {
		eventTickerSet[m.EventTicker] = struct{}{}
	}
	eventTickers := make([]string, 0, len(eventTickerSet))
	for t := range eventTickerSet {
		eventTickers = append(eventTickers, t)
	}

	// Query which events exist in DB
	existingEvents := make(map[string]struct{})
	rows, err := s.db.QueryContext(ctx,
		"SELECT event_ticker FROM events WHERE event_ticker = ANY($1) AND deleted_at IS NULL",
		pq.Array(eventTickers))
	if err != nil {
		return 0, fmt.Errorf("query existing events: %w", err)
	}
	defer rows.Close()
	for rows.Next() {
		var ticker string
		if err := rows.Scan(&ticker); err != nil {
			return 0, err
		}
		existingEvents[ticker] = struct{}{}
	}
	if err := rows.Err(); err != nil {
		return 0, err
	}
	fmt.Printf("    [DB] found %d/%d parent events in DB\n", len(existingEvents), len(eventTickers))

	// Filter markets to only those with existing parent events
	validMarkets := make([]types.Market, 0, len(markets))
	skipped := 0
	for _, m := range markets {
		if _, exists := existingEvents[m.EventTicker]; exists {
			validMarkets = append(validMarkets, m)
		} else {
			skipped++
		}
	}

	if len(validMarkets) == 0 {
		return skipped, nil
	}

	// Process in batches using bulk INSERT
	for i := 0; i < len(validMarkets); i += MarketBatchSize {
		end := i + MarketBatchSize
		if end > len(validMarkets) {
			end = len(validMarkets)
		}
		batch := validMarkets[i:end]

		if err := s.bulkUpsertMarkets(ctx, batch); err != nil {
			return skipped, err
		}
	}

	return skipped, nil
}

// bulkUpsertMarkets inserts a batch of markets using multi-row VALUES
func (s *Store) bulkUpsertMarkets(ctx context.Context, markets []types.Market) error {
	if len(markets) == 0 {
		return nil
	}

	// Build multi-row VALUES clause
	valueStrings := make([]string, 0, len(markets))
	valueArgs := make([]interface{}, 0, len(markets)*13)

	for i, m := range markets {
		base := i * 13
		valueStrings = append(valueStrings, fmt.Sprintf(
			"($%d, $%d, $%d, $%d, $%d, $%d, $%d, $%d, $%d, $%d, $%d, $%d, $%d, NOW())",
			base+1, base+2, base+3, base+4, base+5, base+6, base+7,
			base+8, base+9, base+10, base+11, base+12, base+13,
		))
		valueArgs = append(valueArgs,
			m.Ticker, m.EventTicker, m.Title, m.Status, m.CloseTime,
			m.YesBid, m.YesAsk, m.NoBid, m.NoAsk, m.LastPrice,
			m.Volume, m.Volume24h, m.OpenInterest,
		)
	}

	query := fmt.Sprintf(`
		INSERT INTO markets (ticker, event_ticker, title, status, close_time,
			yes_bid, yes_ask, no_bid, no_ask, last_price, volume, volume_24h, open_interest, updated_at)
		VALUES %s
		ON CONFLICT (ticker) DO UPDATE SET
			title = EXCLUDED.title,
			status = EXCLUDED.status,
			close_time = EXCLUDED.close_time,
			yes_bid = EXCLUDED.yes_bid,
			yes_ask = EXCLUDED.yes_ask,
			no_bid = EXCLUDED.no_bid,
			no_ask = EXCLUDED.no_ask,
			last_price = EXCLUDED.last_price,
			volume = EXCLUDED.volume,
			volume_24h = EXCLUDED.volume_24h,
			open_interest = EXCLUDED.open_interest,
			updated_at = NOW(),
			deleted_at = NULL
	`, strings.Join(valueStrings, ", "))

	result, err := s.db.ExecContext(ctx, query, valueArgs...)
	if err != nil {
		return err
	}
	rows, _ := result.RowsAffected()
	fmt.Printf("    [DB] markets batch: %d attempted, %d affected\n", len(markets), rows)
	return nil
}

// isForeignKeyError checks if error is a postgres FK violation
func isForeignKeyError(err error) bool {
	if err == nil {
		return false
	}
	return strings.Contains(err.Error(), "violates foreign key constraint")
}

// MarketListOptions for filtering markets
type MarketListOptions struct {
	Category      string
	Status        string
	Series        string
	ClosingBefore *time.Time
	ClosingAfter  *time.Time
	Limit         int
}

// ListMarkets returns markets with optional filters
func (s *Store) ListMarkets(ctx context.Context, opts MarketListOptions) ([]types.MarketWithEvent, error) {
	query := `
		SELECT m.ticker, m.event_ticker, m.title, m.status, m.close_time,
			m.yes_bid, m.yes_ask, m.no_bid, m.no_ask, m.last_price,
			m.volume, m.volume_24h, m.open_interest, m.created_at, m.updated_at,
			e.category, e.series_ticker, e.title as event_title
		FROM markets m
		JOIN events e ON m.event_ticker = e.event_ticker
		WHERE m.deleted_at IS NULL AND e.deleted_at IS NULL
	`
	args := []interface{}{}
	argNum := 1

	if opts.Category != "" {
		query += fmt.Sprintf(" AND e.category = $%d", argNum)
		args = append(args, opts.Category)
		argNum++
	}
	if opts.Status != "" {
		query += fmt.Sprintf(" AND m.status = $%d", argNum)
		args = append(args, opts.Status)
		argNum++
	}
	if opts.Series != "" {
		query += fmt.Sprintf(" AND e.series_ticker = $%d", argNum)
		args = append(args, opts.Series)
		argNum++
	}
	if opts.ClosingBefore != nil {
		query += fmt.Sprintf(" AND m.close_time < $%d", argNum)
		args = append(args, opts.ClosingBefore)
		argNum++
	}
	if opts.ClosingAfter != nil {
		query += fmt.Sprintf(" AND m.close_time > $%d", argNum)
		args = append(args, opts.ClosingAfter)
		argNum++
	}

	query += " ORDER BY m.close_time ASC NULLS LAST"

	if opts.Limit > 0 {
		query += fmt.Sprintf(" LIMIT $%d", argNum)
		args = append(args, opts.Limit)
	}

	rows, err := s.db.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var markets []types.MarketWithEvent
	for rows.Next() {
		var m types.MarketWithEvent
		err := rows.Scan(
			&m.Ticker, &m.EventTicker, &m.Title, &m.Status, &m.CloseTime,
			&m.YesBid, &m.YesAsk, &m.NoBid, &m.NoAsk, &m.LastPrice,
			&m.Volume, &m.Volume24h, &m.OpenInterest, &m.CreatedAt, &m.UpdatedAt,
			&m.Category, &m.SeriesTicker, &m.EventTitle,
		)
		if err != nil {
			return nil, err
		}
		markets = append(markets, m)
	}
	return markets, rows.Err()
}

// GetMarket returns a single market by ticker
func (s *Store) GetMarket(ctx context.Context, ticker string) (*types.MarketWithEvent, error) {
	query := `
		SELECT m.ticker, m.event_ticker, m.title, m.status, m.close_time,
			m.yes_bid, m.yes_ask, m.no_bid, m.no_ask, m.last_price,
			m.volume, m.volume_24h, m.open_interest, m.created_at, m.updated_at,
			e.category, e.series_ticker, e.title as event_title
		FROM markets m
		JOIN events e ON m.event_ticker = e.event_ticker
		WHERE m.ticker = $1 AND m.deleted_at IS NULL
	`
	var m types.MarketWithEvent
	err := s.db.QueryRowContext(ctx, query, ticker).Scan(
		&m.Ticker, &m.EventTicker, &m.Title, &m.Status, &m.CloseTime,
		&m.YesBid, &m.YesAsk, &m.NoBid, &m.NoAsk, &m.LastPrice,
		&m.Volume, &m.Volume24h, &m.OpenInterest, &m.CreatedAt, &m.UpdatedAt,
		&m.Category, &m.SeriesTicker, &m.EventTitle,
	)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	return &m, err
}

// GetFees returns fees for a tier
func (s *Store) GetFees(ctx context.Context, tier string) (*types.Fee, error) {
	query := `SELECT tier, maker_fee, taker_fee, updated_at FROM fees WHERE tier = $1`
	var f types.Fee
	err := s.db.QueryRowContext(ctx, query, tier).Scan(&f.Tier, &f.MakerFee, &f.TakerFee, &f.UpdatedAt)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	return &f, err
}

// SecmasterStats holds summary statistics
type SecmasterStats struct {
	TotalEvents        int
	TotalMarkets       int
	MarketsByStatus    map[string]int
	TopCategories      []CategoryCount
	LastSyncTime       *time.Time
	MarketsClosingSoon int // closing within 24h
}

// CategoryCount holds category name and count
type CategoryCount struct {
	Category string
	Count    int
}

// GetStats returns summary statistics about synced data
func (s *Store) GetStats(ctx context.Context) (*SecmasterStats, error) {
	stats := &SecmasterStats{
		MarketsByStatus: make(map[string]int),
	}

	// Total events
	err := s.db.QueryRowContext(ctx, `SELECT COUNT(*) FROM events WHERE deleted_at IS NULL`).Scan(&stats.TotalEvents)
	if err != nil {
		return nil, fmt.Errorf("count events: %w", err)
	}

	// Total markets
	err = s.db.QueryRowContext(ctx, `SELECT COUNT(*) FROM markets WHERE deleted_at IS NULL`).Scan(&stats.TotalMarkets)
	if err != nil {
		return nil, fmt.Errorf("count markets: %w", err)
	}

	// Markets by status
	rows, err := s.db.QueryContext(ctx, `
		SELECT status, COUNT(*) FROM markets
		WHERE deleted_at IS NULL
		GROUP BY status ORDER BY COUNT(*) DESC
	`)
	if err != nil {
		return nil, fmt.Errorf("count by status: %w", err)
	}
	defer rows.Close()
	for rows.Next() {
		var status string
		var count int
		if err := rows.Scan(&status, &count); err != nil {
			return nil, err
		}
		stats.MarketsByStatus[status] = count
	}

	// Top categories (limit 5)
	rows, err = s.db.QueryContext(ctx, `
		SELECT e.category, COUNT(m.ticker) as cnt
		FROM markets m
		JOIN events e ON m.event_ticker = e.event_ticker
		WHERE m.deleted_at IS NULL AND e.deleted_at IS NULL
		GROUP BY e.category
		ORDER BY cnt DESC
		LIMIT 5
	`)
	if err != nil {
		return nil, fmt.Errorf("top categories: %w", err)
	}
	defer rows.Close()
	for rows.Next() {
		var cc CategoryCount
		if err := rows.Scan(&cc.Category, &cc.Count); err != nil {
			return nil, err
		}
		stats.TopCategories = append(stats.TopCategories, cc)
	}

	// Last sync time (most recent updated_at)
	var lastSync sql.NullTime
	err = s.db.QueryRowContext(ctx, `SELECT MAX(updated_at) FROM markets WHERE deleted_at IS NULL`).Scan(&lastSync)
	if err != nil {
		return nil, fmt.Errorf("last sync: %w", err)
	}
	if lastSync.Valid {
		stats.LastSyncTime = &lastSync.Time
	}

	// Markets closing within 24h
	err = s.db.QueryRowContext(ctx, `
		SELECT COUNT(*) FROM markets
		WHERE deleted_at IS NULL
		AND status = 'open'
		AND close_time > NOW()
		AND close_time < NOW() + INTERVAL '24 hours'
	`).Scan(&stats.MarketsClosingSoon)
	if err != nil {
		return nil, fmt.Errorf("closing soon: %w", err)
	}

	return stats, nil
}

// EventListOptions for filtering events
type EventListOptions struct {
	Category string
	Status   string
	Series   string
	Limit    int
}

// ListEvents returns events with market count
func (s *Store) ListEvents(ctx context.Context, opts EventListOptions) ([]types.EventWithMarketCount, error) {
	query := `
		SELECT e.event_ticker, e.title, e.category, e.series_ticker, e.strike_date,
			e.mutually_exclusive, e.status, e.created_at, e.updated_at,
			COUNT(m.ticker) as market_count
		FROM events e
		LEFT JOIN markets m ON e.event_ticker = m.event_ticker AND m.deleted_at IS NULL
		WHERE e.deleted_at IS NULL
	`
	args := []interface{}{}
	argNum := 1

	if opts.Category != "" {
		query += fmt.Sprintf(" AND e.category = $%d", argNum)
		args = append(args, opts.Category)
		argNum++
	}
	if opts.Status != "" {
		query += fmt.Sprintf(" AND e.status = $%d", argNum)
		args = append(args, opts.Status)
		argNum++
	}
	if opts.Series != "" {
		query += fmt.Sprintf(" AND e.series_ticker = $%d", argNum)
		args = append(args, opts.Series)
		argNum++
	}

	query += " GROUP BY e.event_ticker ORDER BY e.strike_date ASC NULLS LAST"

	if opts.Limit > 0 {
		query += fmt.Sprintf(" LIMIT $%d", argNum)
		args = append(args, opts.Limit)
	}

	rows, err := s.db.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var events []types.EventWithMarketCount
	for rows.Next() {
		var e types.EventWithMarketCount
		err := rows.Scan(
			&e.EventTicker, &e.Title, &e.Category, &e.SeriesTicker, &e.StrikeDate,
			&e.MutuallyExclusive, &e.Status, &e.CreatedAt, &e.UpdatedAt,
			&e.MarketCount,
		)
		if err != nil {
			return nil, err
		}
		events = append(events, e)
	}
	return events, rows.Err()
}

// GetEvent returns an event with its markets
func (s *Store) GetEvent(ctx context.Context, eventTicker string) (*types.EventWithMarkets, error) {
	// Get event
	eventQuery := `
		SELECT event_ticker, title, category, series_ticker, strike_date,
			mutually_exclusive, status, created_at, updated_at
		FROM events
		WHERE event_ticker = $1 AND deleted_at IS NULL
	`
	var e types.EventWithMarkets
	err := s.db.QueryRowContext(ctx, eventQuery, eventTicker).Scan(
		&e.EventTicker, &e.Title, &e.Category, &e.SeriesTicker, &e.StrikeDate,
		&e.MutuallyExclusive, &e.Status, &e.CreatedAt, &e.UpdatedAt,
	)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}

	// Get markets for this event
	marketsQuery := `
		SELECT ticker, event_ticker, title, status, close_time,
			yes_bid, yes_ask, no_bid, no_ask, last_price,
			volume, volume_24h, open_interest, created_at, updated_at
		FROM markets
		WHERE event_ticker = $1 AND deleted_at IS NULL
		ORDER BY close_time ASC NULLS LAST
	`
	rows, err := s.db.QueryContext(ctx, marketsQuery, eventTicker)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	for rows.Next() {
		var m types.Market
		err := rows.Scan(
			&m.Ticker, &m.EventTicker, &m.Title, &m.Status, &m.CloseTime,
			&m.YesBid, &m.YesAsk, &m.NoBid, &m.NoAsk, &m.LastPrice,
			&m.Volume, &m.Volume24h, &m.OpenInterest, &m.CreatedAt, &m.UpdatedAt,
		)
		if err != nil {
			return nil, err
		}
		e.Markets = append(e.Markets, m)
	}

	return &e, rows.Err()
}
