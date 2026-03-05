-- migrate:up
ALTER TABLE markets
  ADD COLUMN expected_expiration_time TIMESTAMPTZ;

-- migrate:down
ALTER TABLE markets
  DROP COLUMN IF EXISTS expected_expiration_time;
