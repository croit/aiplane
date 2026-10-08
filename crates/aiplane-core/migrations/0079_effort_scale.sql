-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright (C) 2026 croit GmbH
--
-- One effort scale, `off · low · medium · high · xhigh`, in place of
-- Fast / Standard / Deep / Max. Every stored level moves to the level that
-- thinks as much as it did; conversations without a stored level take the new
-- default, `low`.
UPDATE chat_session_settings SET effort = CASE effort
    WHEN 'fast' THEN 'off'
    WHEN 'standard' THEN 'medium'
    WHEN 'deep' THEN 'high'
    WHEN 'max' THEN 'xhigh'
    ELSE effort
END;

ALTER TABLE model_defaults RENAME COLUMN thinking_budget_standard TO thinking_budget_medium;
ALTER TABLE model_defaults RENAME COLUMN thinking_budget_deep TO thinking_budget_high;
ALTER TABLE model_defaults RENAME COLUMN thinking_budget_max TO thinking_budget_xhigh;
ALTER TABLE model_defaults ADD COLUMN thinking_budget_low INTEGER;

ALTER TABLE model_defaults RENAME COLUMN reasoning_effort_standard TO reasoning_effort_medium;
ALTER TABLE model_defaults RENAME COLUMN reasoning_effort_deep TO reasoning_effort_high;
ALTER TABLE model_defaults RENAME COLUMN reasoning_effort_max TO reasoning_effort_xhigh;
ALTER TABLE model_defaults ADD COLUMN reasoning_effort_low TEXT;

-- How the server caps thinking tokens per request ('token_budget' for vLLM,
-- 'custom_params' for SGLang started with --enable-strict-thinking), learned
-- at identification. NULL = it cannot, and no budget is sent.
ALTER TABLE backend_detected ADD COLUMN thinking_budget TEXT;
