// internal/secmaster/store.go
package secmaster

import (
	"context"
	"database/sql"
	"fmt"
	"time"

	"github.com/aaronwald/ssmd/internal/types"
)

// Store handles secmaster database operations
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
