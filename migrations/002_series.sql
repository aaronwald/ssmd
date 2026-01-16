-- migrations/002_series.sql
-- Series table for Kalshi series-based sync

CREATE TABLE IF NOT EXISTS series (
    ticker VARCHAR(64) PRIMARY KEY,
    title TEXT NOT NULL,
    category VARCHAR(64) NOT NULL,
    tags TEXT[], -- Array of tags from Kalshi API (e.g., ["Basketball", "Pro Basketball"])
    is_game BOOLEAN NOT NULL DEFAULT false, -- For Sports: ticker contains GAME or MATCH
    active BOOLEAN NOT NULL DEFAULT true, -- Soft disable for filtering
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Query by category
CREATE INDEX idx_series_category ON series(category) WHERE active = true;

-- Query by category + games filter (for Sports)
CREATE INDEX idx_series_category_game ON series(category, is_game) WHERE active = true;

-- Query by tag (for Temporal job filtering) - GIN index for array containment
CREATE INDEX idx_series_tags ON series USING GIN(tags) WHERE active = true;
