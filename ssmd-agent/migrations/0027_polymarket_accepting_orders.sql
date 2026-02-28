-- migrate:up
ALTER TABLE polymarket_conditions ADD COLUMN accepting_orders BOOLEAN;

-- Update the updated_at trigger to include accepting_orders in change detection
CREATE OR REPLACE FUNCTION polymarket_conditions_update_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    IF (OLD.status, OLD.active, OLD.accepting_orders, OLD.volume, OLD.liquidity, OLD.winning_outcome, OLD.deleted_at)
       IS DISTINCT FROM
       (NEW.status, NEW.active, NEW.accepting_orders, NEW.volume, NEW.liquidity, NEW.winning_outcome, NEW.deleted_at)
    THEN
        NEW.updated_at = NOW();
    ELSE
        NEW.updated_at = OLD.updated_at;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- migrate:down
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

ALTER TABLE polymarket_conditions DROP COLUMN IF EXISTS accepting_orders;
