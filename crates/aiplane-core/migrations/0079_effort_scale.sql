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

-- Why the gateway stopped a model call: 'loop' (repeated text) or
-- 'repeated_call' (the same tool call over and over). NULL = it was not
-- stopped. Recorded on the chat and the /v1 path alike, so loops can be
-- counted wherever they happen.
ALTER TABLE usage_events ADD COLUMN stop_reason TEXT;

-- A model call of a chat turn that looped and was retried at a lower effort.
-- Its partial output moves here so the turn shows it collapsed above the
-- answer and the model never sees it again. `retry_effort` is the level the
-- next try ran at; NULL on the last attempt of a turn that gave up.
CREATE TABLE chat_turn_attempts (
    turn_id      TEXT NOT NULL REFERENCES chat_turns(id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    effort       TEXT NOT NULL,
    retry_effort TEXT,
    stop_reason  TEXT NOT NULL,
    reasoning    TEXT NOT NULL,
    content      TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (turn_id, seq)
) STRICT;

-- A machine-readable reason next to `error_message`, so the SPA can say it in
-- the reader's language. 'loop_exhausted' = every retry looped too.
ALTER TABLE chat_turns ADD COLUMN error_code TEXT;
