-- migrate:up
ALTER TABLE api_keys ADD COLUMN allowed_feeds TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE api_keys ADD COLUMN date_range_start DATE NOT NULL DEFAULT '1970-01-01';
ALTER TABLE api_keys ADD COLUMN date_range_end DATE NOT NULL DEFAULT '1970-01-01';

-- Revoke all existing keys (clean slate — new keys will have restrictions)
UPDATE api_keys SET revoked_at = NOW() WHERE revoked_at IS NULL;

-- GIN index for array containment queries
CREATE INDEX idx_api_keys_allowed_feeds ON api_keys USING gin(allowed_feeds);

-- migrate:down
DROP INDEX IF EXISTS idx_api_keys_allowed_feeds;
ALTER TABLE api_keys DROP COLUMN IF EXISTS date_range_end;
ALTER TABLE api_keys DROP COLUMN IF EXISTS date_range_start;
ALTER TABLE api_keys DROP COLUMN IF EXISTS allowed_feeds;
