-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright (C) 2026 croit GmbH
--
-- What kind of server each backend turned out to be, and what it told us
-- about itself.
--
-- The gateway routes to anything that speaks the OpenAI wire. That is the
-- right abstraction for requests and the wrong one for two facts a request
-- depends on: how much context a model has, and how this server spells
-- "think harder". Both were answered by assuming vLLM -- the context was
-- read from a vLLM-only field, the reasoning spelling was guessed from the
-- model *name* -- so an Ollama backend silently truncated long conversations
-- and silently ignored the effort control. Neither produced an error.
--
-- `upstreams::profile::detect` now identifies the server and reads what it
-- will say. The result is observed state, so it lives here rather than as
-- columns on `backends`, for the same reason `backend_probed_models` is its
-- own table (0062): that row is what the operator declared, and a detection
-- round must never be able to rewrite it.
--
-- Detection runs when the topology is applied and on demand from the admin
-- UI's test button -- not on the recurring 5 s health probe, which stays a
-- liveness + model-set check. These rows are therefore the only thing that
-- knows a backend's profile between an apply and a restart, and seeding from
-- them on boot is what stops the admin page showing "generic" for every
-- backend until someone re-applies.

CREATE TABLE backend_detected (
    backend_id   INTEGER PRIMARY KEY REFERENCES backends(id) ON DELETE CASCADE,
    -- 'generic' | 'vllm' | 'ollama' | 'llamacpp' | 'sglang'. An unknown value
    -- parses back to 'generic', which is the pre-detection behaviour, so a row
    -- written by a newer version degrades instead of breaking.
    profile      TEXT NOT NULL,
    -- The context this server said it actually allocated (llama.cpp's
    -- `/props`), as a ceiling over the per-model windows below. Kept apart from
    -- them because `/models` reports a model's *trained* context, which on
    -- llama.cpp is routinely many times what the server was started with.
    context_cap  INTEGER,
    -- Requests this server said it runs at once (llama.cpp's slot count).
    -- NULL = it did not say, and the operator's `max_inflight` stands.
    max_parallel INTEGER,
    -- The server's own version string. Shown to the operator, never parsed:
    -- behaviour is decided by the profile, not by a version number.
    version      TEXT,
    detected_at  TEXT NOT NULL                  -- RFC3339 UTC
) STRICT;

-- Context window per model, as the server reported it.
--
-- Separate from `model_defaults.context_window`, which is the operator's
-- value. Keeping both is the point: the operator may override, and the UI can
-- only warn that an override exceeds what the server actually serves -- the
-- silent-truncation case -- while it still remembers what was detected.
CREATE TABLE backend_detected_context (
    backend_id     INTEGER NOT NULL REFERENCES backends(id) ON DELETE CASCADE,
    model_id       TEXT NOT NULL,
    context_window INTEGER NOT NULL,
    PRIMARY KEY (backend_id, model_id)
) STRICT;
