-- migrate:up
ALTER TABLE api_keys ADD COLUMN expires_at TIMESTAMPTZ;
CREATE INDEX idx_api_keys_expires ON api_keys(expires_at) WHERE revoked_at IS NULL;

CREATE TABLE data_access_log (
  id BIGSERIAL PRIMARY KEY,
  key_prefix VARCHAR(30) NOT NULL,
  user_email VARCHAR(255) NOT NULL,
  feed VARCHAR(64) NOT NULL,
  date_from DATE NOT NULL,
  date_to DATE NOT NULL,
  msg_type VARCHAR(64),
  files_count INTEGER NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_data_access_log_created ON data_access_log(created_at);
CREATE INDEX idx_data_access_log_user ON data_access_log(user_email, created_at);

-- migrate:down
DROP TABLE IF EXISTS data_access_log;
DROP INDEX IF EXISTS idx_api_keys_expires;
ALTER TABLE api_keys DROP COLUMN IF EXISTS expires_at;
