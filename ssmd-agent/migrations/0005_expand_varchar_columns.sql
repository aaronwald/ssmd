-- migrate:up
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

-- migrate:down
-- Note: Shrinking columns could fail if data exceeds 64 chars
ALTER TABLE series_fees ALTER COLUMN series_ticker TYPE varchar(64);
ALTER TABLE series ALTER COLUMN category TYPE varchar(64);
ALTER TABLE series ALTER COLUMN ticker TYPE varchar(64);
ALTER TABLE markets ALTER COLUMN event_ticker TYPE varchar(64);
ALTER TABLE markets ALTER COLUMN ticker TYPE varchar(64);
ALTER TABLE events ALTER COLUMN series_ticker TYPE varchar(64);
ALTER TABLE events ALTER COLUMN category TYPE varchar(64);
ALTER TABLE events ALTER COLUMN event_ticker TYPE varchar(64);
