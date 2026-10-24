-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright (C) 2026 croit GmbH

-- Cache-aware cost accounting.
--
-- Upstream providers (OpenRouter, Anthropic, OpenAI) bill prompt tokens that
-- hit their prompt cache at a steep discount — for MiniMax models on
-- OpenRouter a cache read costs ~80-90% less than the same token uncached.
-- The usage rows recorded the full `prompt_tokens` count and the cost
-- settlement priced every one of them at the input rate, so a cached
-- workload's spend was overstated by several times.
--
-- `usage_events.cached_tokens` (and the `usage_daily` rollup) carry the
-- `usage.prompt_tokens_details.cached_tokens` count the upstream reported;
-- `model_defaults.cache_read_price` is the optional per-1M price an operator
-- sets for those tokens. Settlement prices `prompt − cached` at the input
-- rate and `cached` at the cache-read rate; with no cache-read price the
-- cached tokens fall back to the input rate (today's behaviour).

ALTER TABLE usage_events ADD COLUMN cached_tokens INTEGER;
ALTER TABLE usage_daily ADD COLUMN cached_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE model_defaults ADD COLUMN cache_read_price REAL;
