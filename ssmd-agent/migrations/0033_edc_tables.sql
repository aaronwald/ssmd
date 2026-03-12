-- migrate:up
CREATE TABLE edc_changelog_snapshots (
    id SERIAL PRIMARY KEY,
    exchange TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    raw_text TEXT NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_edc_snapshots_exchange ON edc_changelog_snapshots (exchange, fetched_at DESC);

CREATE TABLE edc_memories (
    id SERIAL PRIMARY KEY,
    exchange TEXT NOT NULL,
    changelog_summary TEXT NOT NULL,
    impact TEXT NOT NULL,
    affected_components TEXT NOT NULL,
    fix_description TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_edc_memories_exchange ON edc_memories (exchange, created_at DESC);

GRANT SELECT ON edc_changelog_snapshots TO pipeline_readonly;
GRANT SELECT ON edc_memories TO pipeline_readonly;

-- migrate:down
DROP TABLE IF EXISTS edc_memories;
DROP TABLE IF EXISTS edc_changelog_snapshots;
