package types

import "time"

// MarketStatus represents the status of a market
type MarketStatus string

const (
	MarketStatusOpen    MarketStatus = "open"
	MarketStatusClosed  MarketStatus = "closed"
	MarketStatusSettled MarketStatus = "settled"
)

// Event represents a Kalshi event (container for markets)
type Event struct {
	EventTicker       string     `json:"event_ticker" db:"event_ticker"`
	Title             string     `json:"title" db:"title"`
	Category          string     `json:"category" db:"category"`
	SeriesTicker      string     `json:"series_ticker" db:"series_ticker"`
	StrikeDate        *time.Time `json:"strike_date,omitempty" db:"strike_date"`
	MutuallyExclusive bool       `json:"mutually_exclusive" db:"mutually_exclusive"`
	Status            string     `json:"status" db:"status"`
	CreatedAt         time.Time  `json:"created_at" db:"created_at"`
	UpdatedAt         time.Time  `json:"updated_at" db:"updated_at"`
}

// Market represents a Kalshi market (tradeable contract)
type Market struct {
	Ticker       string       `json:"ticker" db:"ticker"`
	EventTicker  string       `json:"event_ticker" db:"event_ticker"`
	Title        string       `json:"title" db:"title"`
	Status       MarketStatus `json:"status" db:"status"`
	CloseTime    *time.Time   `json:"close_time,omitempty" db:"close_time"`
	YesBid       *int         `json:"yes_bid,omitempty" db:"yes_bid"`
	YesAsk       *int         `json:"yes_ask,omitempty" db:"yes_ask"`
	NoBid        *int         `json:"no_bid,omitempty" db:"no_bid"`
	NoAsk        *int         `json:"no_ask,omitempty" db:"no_ask"`
	LastPrice    *int         `json:"last_price,omitempty" db:"last_price"`
	Volume       *int64       `json:"volume,omitempty" db:"volume"`
	Volume24h    *int64       `json:"volume_24h,omitempty" db:"volume_24h"`
	OpenInterest *int64       `json:"open_interest,omitempty" db:"open_interest"`
	CreatedAt    time.Time    `json:"created_at" db:"created_at"`
	UpdatedAt    time.Time    `json:"updated_at" db:"updated_at"`
}

// MarketWithEvent is a market joined with event metadata
type MarketWithEvent struct {
	Market
	Category     string `json:"category" db:"category"`
	SeriesTicker string `json:"series_ticker" db:"series_ticker"`
	EventTitle   string `json:"event_title" db:"event_title"`
}

// Fee represents fee schedule for a tier
type Fee struct {
	Tier      string    `json:"tier" db:"tier"`
	MakerFee  float64   `json:"maker_fee" db:"maker_fee"`
	TakerFee  float64   `json:"taker_fee" db:"taker_fee"`
	UpdatedAt time.Time `json:"updated_at" db:"updated_at"`
}
