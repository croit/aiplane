-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright (C) 2026 croit GmbH
--
-- The last model set each backend was seen serving, so an outage never looks
-- like a typo.
--
-- Which models exist is discovered from each backend's `/models` probe and,
-- until now, lived only in memory. That is fine while the process is up: a
-- backend that dies keeps its discovered set, so `knows_model` stays true and
-- the router answers 503 ("known, but every replica is down"). It falls apart
-- across a restart. Boot the gateway while a backend is down and no probe ever
-- succeeds, so the set is empty, the model is unknown to every pool, and the
-- router answers 404 `model_not_found` -- which clients read as "that model
-- does not exist or you do not have access to it" and do NOT retry. The same
-- gap drops the model out of `GET /v1/models` entirely.
--
-- So remember what each backend last advertised. On boot the registry seeds
-- each backend's model set from here, which restores exactly the in-process
-- behaviour: the model is *known* (listed, 503 while down) without being
-- *routable* (health still gates that, and health is only ever granted by a
-- live probe). The probe overwrites these rows the moment it succeeds, so a
-- backend whose loadout changed while the gateway was down self-corrects on its
-- first successful probe.
--
-- Deliberately a separate table from `backend_models`: that one is the
-- operator's static allowlist/fallback (config, edited in the UI), this one is
-- observed state the gateway owns. Merging them would let a probe silently
-- rewrite what the operator declared.

CREATE TABLE backend_probed_models (
    backend_name TEXT NOT NULL REFERENCES backends(name) ON DELETE CASCADE,
    model_id     TEXT NOT NULL,
    -- When this set was last written, for operator debugging ("these models are
    -- from a probe three weeks ago"). Not read by the routing path.
    seen_at      TEXT NOT NULL,
    PRIMARY KEY (backend_name, model_id)
) STRICT;
