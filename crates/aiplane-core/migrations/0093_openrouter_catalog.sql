-- OpenRouter as a first-class provider type: one backend, one key, a catalog
-- read from the account, and a per-model purchase-price ceiling that the
-- gateway enforces on every request.

ALTER TABLE backends ADD COLUMN provider TEXT NOT NULL DEFAULT 'openai_compatible';

-- One row per selected OpenRouter model. The selected set itself lives in
-- `pool_models` (per pool, so one backend can serve chat, embeddings, speech
-- and System One with a different selection in each); this table carries the
-- policy that applies wherever that model is routed.
CREATE TABLE openrouter_model_policies (
    backend_id           INTEGER NOT NULL REFERENCES backends(id) ON DELETE CASCADE,
    model_id             TEXT NOT NULL,
    -- USD per 1M tokens, the unit OpenRouter's `provider.max_price` uses.
    -- Both are NOT NULL: a selected model without a ceiling would be exactly
    -- the unbounded spend this table exists to prevent.
    max_prompt_price     REAL NOT NULL,
    max_completion_price REAL NOT NULL,
    -- cheapest | fastest | balanced
    strategy             TEXT NOT NULL DEFAULT 'balanced',
    -- Preferred minimum throughput (tokens/s) for `cheapest`; NULL = no floor.
    min_throughput       REAL,
    -- Whether at least one provider currently satisfies both ceilings. Kept
    -- here so the request path and /v1/models agree with the last catalog
    -- refresh without another query per model.
    available            INTEGER NOT NULL DEFAULT 1,
    cheapest_prompt_price     REAL,
    cheapest_completion_price REAL,
    cheapest_provider         TEXT,
    observed_at          TEXT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    PRIMARY KEY (backend_id, model_id)
) STRICT;

-- The request path resolves a policy by model id alone (OpenRouter ids are
-- globally unique), so that lookup must not scan the table on every request.
CREATE INDEX openrouter_model_policies_model
    ON openrouter_model_policies(model_id);

-- What each provider charges for a selected model right now, from
-- `GET /models/{author}/{slug}/endpoints`. The request path uses the ceiling
-- in `openrouter_model_policies`; these rows are what the admin page shows and
-- what the ceiling is checked against on refresh.
CREATE TABLE openrouter_endpoint_observations (
    backend_id             INTEGER NOT NULL REFERENCES backends(id) ON DELETE CASCADE,
    model_id               TEXT NOT NULL,
    provider_tag           TEXT NOT NULL,
    provider_name          TEXT NOT NULL,
    prompt_price           REAL NOT NULL,
    completion_price       REAL NOT NULL,
    request_price          REAL,
    image_price            REAL,
    cache_read_price       REAL,
    cache_write_price      REAL,
    context_length         INTEGER,
    max_completion_tokens  INTEGER,
    throughput_p50         REAL,
    latency_p50            REAL,
    uptime_30m             REAL,
    quantization           TEXT,
    supported_parameters   TEXT NOT NULL DEFAULT '[]',
    observed_at            TEXT NOT NULL,
    PRIMARY KEY (backend_id, model_id, provider_tag)
) STRICT;
