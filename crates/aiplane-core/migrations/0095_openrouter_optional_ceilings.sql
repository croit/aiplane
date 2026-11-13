-- A price ceiling is a filter, not a requirement: a side left empty accepts
-- every provider, and the model's strategy picks among them. SQLite cannot
-- drop NOT NULL in place, so the table is rebuilt with the same rows.
--
-- `price_unit` records what the ceilings and observed prices are billed per:
-- USD per 1M tokens for chat and token-priced transcription, USD per audio
-- second for whisper-class transcription. Rebuilding is safe here: no other
-- table references this one.
CREATE TABLE openrouter_model_policies_new (
    backend_id           INTEGER NOT NULL REFERENCES backends(id) ON DELETE CASCADE,
    model_id             TEXT NOT NULL,
    max_prompt_price     REAL,
    max_completion_price REAL,
    price_unit           TEXT NOT NULL DEFAULT 'token'
                         CHECK (price_unit IN ('token', 'second')),
    strategy             TEXT NOT NULL DEFAULT 'balanced',
    min_throughput       REAL,
    available            INTEGER NOT NULL DEFAULT 1,
    cheapest_prompt_price     REAL,
    cheapest_completion_price REAL,
    cheapest_provider         TEXT,
    observed_at          TEXT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    PRIMARY KEY (backend_id, model_id)
) STRICT;

INSERT INTO openrouter_model_policies_new
    (backend_id, model_id, max_prompt_price, max_completion_price, price_unit,
     strategy, min_throughput, available, cheapest_prompt_price,
     cheapest_completion_price, cheapest_provider, observed_at, created_at, updated_at)
SELECT backend_id, model_id, max_prompt_price, max_completion_price, 'token',
       strategy, min_throughput, available, cheapest_prompt_price,
       cheapest_completion_price, cheapest_provider, observed_at, created_at, updated_at
  FROM openrouter_model_policies;

DROP TABLE openrouter_model_policies;
ALTER TABLE openrouter_model_policies_new RENAME TO openrouter_model_policies;

CREATE INDEX openrouter_model_policies_model
    ON openrouter_model_policies(model_id);
