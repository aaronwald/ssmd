-- migrations/002_series_fees.sql
-- Kalshi series fee schedules with time-travel support

BEGIN;

-- Enable btree_gist for exclusion constraint
CREATE EXTENSION IF NOT EXISTS btree_gist;

-- Fee type enum matching Kalshi API
CREATE TYPE fee_type AS ENUM ('quadratic', 'quadratic_with_maker_fees', 'flat');

-- Series fee schedules with temporal validity
-- Each row represents a fee configuration effective for a time range
CREATE TABLE series_fees (
    id SERIAL PRIMARY KEY,
    series_ticker VARCHAR(64) NOT NULL,
    fee_type fee_type NOT NULL,
    fee_multiplier NUMERIC(6,4) NOT NULL DEFAULT 1.0,
    effective_from TIMESTAMPTZ NOT NULL,
    effective_to TIMESTAMPTZ,  -- NULL = currently active
    source_id VARCHAR(128),    -- Kalshi API ID for deduplication
    created_at TIMESTAMPTZ DEFAULT NOW(),

    -- Prevent overlapping fee periods for the same series
    CONSTRAINT no_overlapping_fees EXCLUDE USING gist (
        series_ticker WITH =,
        tstzrange(effective_from, COALESCE(effective_to, 'infinity'::timestamptz), '[)') WITH &&
    )
);

-- Index for series lookups
CREATE INDEX idx_series_fees_ticker ON series_fees(series_ticker);

-- Index for point-in-time queries (as_of)
CREATE INDEX idx_series_fees_effective ON series_fees(effective_from, effective_to);

-- Index for deduplication by source_id
CREATE UNIQUE INDEX idx_series_fees_source_id ON series_fees(source_id) WHERE source_id IS NOT NULL;

-- Comments for documentation
COMMENT ON TABLE series_fees IS 'Kalshi series fee schedules with time-travel support. Use effective_from/to for point-in-time queries.';
COMMENT ON COLUMN series_fees.fee_type IS 'quadratic=taker only, quadratic_with_maker_fees=both, flat=per-contract';
COMMENT ON COLUMN series_fees.fee_multiplier IS 'Series-specific multiplier applied to base fee formula';
COMMENT ON COLUMN series_fees.effective_to IS 'NULL means currently active, set when superseded';
COMMENT ON COLUMN series_fees.source_id IS 'Kalshi API fee change ID for deduplication';

COMMIT;
