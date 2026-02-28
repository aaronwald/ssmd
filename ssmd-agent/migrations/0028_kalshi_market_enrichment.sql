-- migrate:up
-- Add Kalshi market enrichment fields: strikes, result, subtitles, market type, open time

ALTER TABLE markets
    ADD COLUMN floor_strike NUMERIC,
    ADD COLUMN cap_strike NUMERIC,
    ADD COLUMN strike_type VARCHAR(32),
    ADD COLUMN result VARCHAR(16),
    ADD COLUMN expiration_value TEXT,
    ADD COLUMN yes_sub_title TEXT,
    ADD COLUMN no_sub_title TEXT,
    ADD COLUMN can_close_early BOOLEAN,
    ADD COLUMN market_type VARCHAR(16),
    ADD COLUMN open_time TIMESTAMPTZ;

-- Update the updated_at trigger to include new fields in change detection
CREATE OR REPLACE FUNCTION markets_update_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    IF (OLD.status, OLD.close_time, OLD.yes_bid, OLD.yes_ask, OLD.no_bid, OLD.no_ask,
        OLD.last_price, OLD.volume, OLD.volume_24h, OLD.open_interest, OLD.deleted_at,
        OLD.result, OLD.expiration_value, OLD.can_close_early)
       IS DISTINCT FROM
       (NEW.status, NEW.close_time, NEW.yes_bid, NEW.yes_ask, NEW.no_bid, NEW.no_ask,
        NEW.last_price, NEW.volume, NEW.volume_24h, NEW.open_interest, NEW.deleted_at,
        NEW.result, NEW.expiration_value, NEW.can_close_early)
    THEN
        NEW.updated_at = NOW();
    ELSE
        NEW.updated_at = OLD.updated_at;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- migrate:down
-- Restore original trigger
CREATE OR REPLACE FUNCTION markets_update_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    IF (OLD.status, OLD.close_time, OLD.yes_bid, OLD.yes_ask, OLD.no_bid, OLD.no_ask,
        OLD.last_price, OLD.volume, OLD.volume_24h, OLD.open_interest, OLD.deleted_at)
       IS DISTINCT FROM
       (NEW.status, NEW.close_time, NEW.yes_bid, NEW.yes_ask, NEW.no_bid, NEW.no_ask,
        NEW.last_price, NEW.volume, NEW.volume_24h, NEW.open_interest, NEW.deleted_at)
    THEN
        NEW.updated_at = NOW();
    ELSE
        NEW.updated_at = OLD.updated_at;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

ALTER TABLE markets
    DROP COLUMN IF EXISTS floor_strike,
    DROP COLUMN IF EXISTS cap_strike,
    DROP COLUMN IF EXISTS strike_type,
    DROP COLUMN IF EXISTS result,
    DROP COLUMN IF EXISTS expiration_value,
    DROP COLUMN IF EXISTS yes_sub_title,
    DROP COLUMN IF EXISTS no_sub_title,
    DROP COLUMN IF EXISTS can_close_early,
    DROP COLUMN IF EXISTS market_type,
    DROP COLUMN IF EXISTS open_time;
