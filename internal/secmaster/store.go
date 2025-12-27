// internal/secmaster/store.go
package secmaster

import (
	"context"
	"database/sql"
	"fmt"
	"strings"
	"time"

	"github.com/aaronwald/ssmd/internal/types"
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

// UpsertEventBatch upserts multiple events in a single transaction
func (s *Store) UpsertEventBatch(ctx context.Context, events []types.Event) error {
	if len(events) == 0 {
		return nil
	}

	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin tx: %w", err)
	}
	defer tx.Rollback()

	stmt, err := tx.PrepareContext(ctx, `
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
	`)
	if err != nil {
		return fmt.Errorf("prepare: %w", err)
	}
	defer stmt.Close()

	for _, e := range events {
		_, err := stmt.ExecContext(ctx, e.EventTicker, e.Title, e.Category, e.SeriesTicker,
			e.StrikeDate, e.MutuallyExclusive, e.Status)
		if err != nil {
			return fmt.Errorf("exec event %s: %w", e.EventTicker, err)
		}
	}

	return tx.Commit()
}

// UpsertMarketBatch upserts multiple markets, skipping those with missing parent events.
// Uses individual statements (not a transaction) because PostgreSQL aborts transactions
// after any error - we need to continue after FK violations.
// Returns count of skipped markets (FK violations) for logging.
func (s *Store) UpsertMarketBatch(ctx context.Context, markets []types.Market) (int, error) {
	if len(markets) == 0 {
		return 0, nil
	}

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

	skipped := 0
	for _, m := range markets {
		_, err := s.db.ExecContext(ctx, query, m.Ticker, m.EventTicker, m.Title, m.Status, m.CloseTime,
			m.YesBid, m.YesAsk, m.NoBid, m.NoAsk, m.LastPrice,
			m.Volume, m.Volume24h, m.OpenInterest)
		if err != nil {
			if isForeignKeyError(err) {
				skipped++
				continue
			}
			return skipped, fmt.Errorf("upsert market %s: %w", m.Ticker, err)
		}
	}

	return skipped, nil
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
