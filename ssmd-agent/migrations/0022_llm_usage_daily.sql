-- migrate:up

-- Durable daily LLM token usage (Redis is hot path, this is archive)
CREATE TABLE llm_usage_daily (
    id                BIGSERIAL PRIMARY KEY,
    key_prefix        VARCHAR(30) NOT NULL,
    date              DATE NOT NULL,
    model             VARCHAR(128) NOT NULL,
    prompt_tokens     BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    requests          INTEGER NOT NULL DEFAULT 0,
    cost_usd          NUMERIC(12, 6) NOT NULL DEFAULT 0,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (key_prefix, date, model)
);

CREATE INDEX idx_llm_usage_daily_billing
    ON llm_usage_daily (key_prefix, date);

-- migrate:down
DROP TABLE IF EXISTS llm_usage_daily;
