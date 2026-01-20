-- Migration: Expand varchar(64) columns to varchar(128) for ticker fields
-- Prevents "value too long for type character varying(64)" errors from Kalshi API

-- Events table
ALTER TABLE events ALTER COLUMN event_ticker TYPE varchar(128);
ALTER TABLE events ALTER COLUMN category TYPE varchar(128);
ALTER TABLE events ALTER COLUMN series_ticker TYPE varchar(128);

-- Markets table
ALTER TABLE markets ALTER COLUMN ticker TYPE varchar(128);
ALTER TABLE markets ALTER COLUMN event_ticker TYPE varchar(128);

-- Series table
ALTER TABLE series ALTER COLUMN ticker TYPE varchar(128);
ALTER TABLE series ALTER COLUMN category TYPE varchar(128);

-- Series fees table
ALTER TABLE series_fees ALTER COLUMN series_ticker TYPE varchar(128);
