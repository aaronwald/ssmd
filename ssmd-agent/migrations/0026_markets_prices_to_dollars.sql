-- migrate:up
-- Convert market price columns from INTEGER (cents 0-100) to NUMERIC (dollars 0.00-1.00).
-- Kalshi is deprecating integer cent fields on March 5 2026 in favor of _dollars fields.
-- Existing cent values are divided by 100 to convert to dollars.

ALTER TABLE markets
    ALTER COLUMN yes_bid TYPE NUMERIC(8, 4) USING (yes_bid::NUMERIC / 100),
    ALTER COLUMN yes_ask TYPE NUMERIC(8, 4) USING (yes_ask::NUMERIC / 100),
    ALTER COLUMN no_bid TYPE NUMERIC(8, 4) USING (no_bid::NUMERIC / 100),
    ALTER COLUMN no_ask TYPE NUMERIC(8, 4) USING (no_ask::NUMERIC / 100),
    ALTER COLUMN last_price TYPE NUMERIC(8, 4) USING (last_price::NUMERIC / 100);

-- migrate:down
-- Convert back from dollars to cents (multiply by 100, round to integer)
ALTER TABLE markets
    ALTER COLUMN yes_bid TYPE INT USING (ROUND(yes_bid * 100))::INT,
    ALTER COLUMN yes_ask TYPE INT USING (ROUND(yes_ask * 100))::INT,
    ALTER COLUMN no_bid TYPE INT USING (ROUND(no_bid * 100))::INT,
    ALTER COLUMN no_ask TYPE INT USING (ROUND(no_ask * 100))::INT,
    ALTER COLUMN last_price TYPE INT USING (ROUND(last_price * 100))::INT;
