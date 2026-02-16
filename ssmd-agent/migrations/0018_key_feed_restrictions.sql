-- migrate:up
ALTER TABLE api_keys ADD COLUMN allowed_feeds TEXT[];
ALTER TABLE api_keys ADD COLUMN date_range_start DATE;
ALTER TABLE api_keys ADD COLUMN date_range_end DATE;

-- Revoke all existing keys (clean slate — new keys will have restrictions)
UPDATE api_keys SET revoked_at = NOW() WHERE revoked_at IS NULL;

-- Now safe to make NOT NULL since no active keys exist
ALTER TABLE api_keys ALTER COLUMN allowed_feeds SET NOT NULL;
ALTER TABLE api_keys ALTER COLUMN date_range_start SET NOT NULL;
ALTER TABLE api_keys ALTER COLUMN date_range_end SET NOT NULL;

-- GIN index for array containment queries
CREATE INDEX idx_api_keys_allowed_feeds ON api_keys USING gin(allowed_feeds);

-- migrate:down
DROP INDEX IF EXISTS idx_api_keys_allowed_feeds;
ALTER TABLE api_keys DROP COLUMN IF EXISTS date_range_end;
ALTER TABLE api_keys DROP COLUMN IF EXISTS date_range_start;
ALTER TABLE api_keys DROP COLUMN IF EXISTS allowed_feeds;
