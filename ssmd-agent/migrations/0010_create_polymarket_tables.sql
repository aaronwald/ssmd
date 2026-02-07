-- migrate:up

CREATE TABLE polymarket_conditions (
    condition_id    VARCHAR(128) PRIMARY KEY,
    question        TEXT NOT NULL,
    slug            VARCHAR(256),
    category        VARCHAR(128),
    outcomes        TEXT[] NOT NULL DEFAULT '{}',
    status          VARCHAR(16) NOT NULL DEFAULT 'active',
    active          BOOLEAN NOT NULL DEFAULT true,
    end_date        TIMESTAMPTZ,
    resolution_date TIMESTAMPTZ,
    winning_outcome VARCHAR(128),
    volume          NUMERIC(24,2),
    liquidity       NUMERIC(24,2),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at      TIMESTAMPTZ
);

CREATE TABLE polymarket_tokens (
    token_id        VARCHAR(128) PRIMARY KEY,
    condition_id    VARCHAR(128) NOT NULL REFERENCES polymarket_conditions(condition_id) ON DELETE CASCADE,
    outcome         VARCHAR(128) NOT NULL,
    outcome_index   INTEGER NOT NULL DEFAULT 0,
    price           NUMERIC(8,4),
    bid             NUMERIC(8,4),
    ask             NUMERIC(8,4),
    volume          NUMERIC(24,2),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes
CREATE INDEX idx_polymarket_conditions_status ON polymarket_conditions(status) WHERE deleted_at IS NULL;
CREATE INDEX idx_polymarket_conditions_category ON polymarket_conditions(category) WHERE deleted_at IS NULL;
CREATE INDEX idx_polymarket_tokens_condition ON polymarket_tokens(condition_id);

-- Updated_at triggers (IS DISTINCT FROM pattern)
CREATE OR REPLACE FUNCTION polymarket_conditions_update_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    IF (OLD.status, OLD.active, OLD.volume, OLD.liquidity, OLD.winning_outcome, OLD.deleted_at)
       IS DISTINCT FROM
       (NEW.status, NEW.active, NEW.volume, NEW.liquidity, NEW.winning_outcome, NEW.deleted_at)
    THEN
        NEW.updated_at = NOW();
    ELSE
        NEW.updated_at = OLD.updated_at;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER update_polymarket_conditions_updated_at
    BEFORE UPDATE ON polymarket_conditions
    FOR EACH ROW
    EXECUTE FUNCTION polymarket_conditions_update_timestamp();

CREATE OR REPLACE FUNCTION polymarket_tokens_update_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    IF (OLD.price, OLD.bid, OLD.ask, OLD.volume)
       IS DISTINCT FROM
       (NEW.price, NEW.bid, NEW.ask, NEW.volume)
    THEN
        NEW.updated_at = NOW();
    ELSE
        NEW.updated_at = OLD.updated_at;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER update_polymarket_tokens_updated_at
    BEFORE UPDATE ON polymarket_tokens
    FOR EACH ROW
    EXECUTE FUNCTION polymarket_tokens_update_timestamp();

-- migrate:down
DROP TRIGGER IF EXISTS update_polymarket_tokens_updated_at ON polymarket_tokens;
DROP FUNCTION IF EXISTS polymarket_tokens_update_timestamp();
DROP TRIGGER IF EXISTS update_polymarket_conditions_updated_at ON polymarket_conditions;
DROP FUNCTION IF EXISTS polymarket_conditions_update_timestamp();
DROP TABLE IF EXISTS polymarket_tokens;
DROP TABLE IF EXISTS polymarket_conditions;
