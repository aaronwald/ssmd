-- Migration: Add trigger to only update updated_at when data actually changes
-- This prevents CDC spam from secmaster syncs that don't modify data

-- Trigger function for events table
CREATE OR REPLACE FUNCTION events_update_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    -- Only update timestamp if actual data changed (exclude updated_at from comparison)
    IF (OLD.title, OLD.category, OLD.series_ticker, OLD.strike_date,
        OLD.mutually_exclusive, OLD.status, OLD.deleted_at)
       IS DISTINCT FROM
       (NEW.title, NEW.category, NEW.series_ticker, NEW.strike_date,
        NEW.mutually_exclusive, NEW.status, NEW.deleted_at) THEN
        NEW.updated_at = NOW();
    ELSE
        NEW.updated_at = OLD.updated_at;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger function for markets table
CREATE OR REPLACE FUNCTION markets_update_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    -- Only update timestamp if actual data changed (exclude updated_at from comparison)
    IF (OLD.event_ticker, OLD.title, OLD.status, OLD.close_time,
        OLD.yes_bid, OLD.yes_ask, OLD.no_bid, OLD.no_ask,
        OLD.last_price, OLD.volume, OLD.volume_24h, OLD.open_interest, OLD.deleted_at)
       IS DISTINCT FROM
       (NEW.event_ticker, NEW.title, NEW.status, NEW.close_time,
        NEW.yes_bid, NEW.yes_ask, NEW.no_bid, NEW.no_ask,
        NEW.last_price, NEW.volume, NEW.volume_24h, NEW.open_interest, NEW.deleted_at) THEN
        NEW.updated_at = NOW();
    ELSE
        NEW.updated_at = OLD.updated_at;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Apply trigger to events table
DROP TRIGGER IF EXISTS events_updated_at_trigger ON events;
CREATE TRIGGER events_updated_at_trigger
    BEFORE UPDATE ON events
    FOR EACH ROW
    EXECUTE FUNCTION events_update_timestamp();

-- Apply trigger to markets table
DROP TRIGGER IF EXISTS markets_updated_at_trigger ON markets;
CREATE TRIGGER markets_updated_at_trigger
    BEFORE UPDATE ON markets
    FOR EACH ROW
    EXECUTE FUNCTION markets_update_timestamp();
