-- migrate:up
ALTER TABLE series ADD COLUMN volume bigint NOT NULL DEFAULT 0;

-- migrate:down
ALTER TABLE series DROP COLUMN volume;
