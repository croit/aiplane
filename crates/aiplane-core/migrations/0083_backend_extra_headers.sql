-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright (C) 2026 croit GmbH

-- Operator-configured extra headers on a backend's outbound requests.
--
-- Some upstream APIs require a header beyond the bearer credential on every
-- request (Anthropic: `anthropic-version`); without it the health probe, the
-- connection test and every chat round fail with a 400 the operator cannot
-- read from the admin UI. A stored JSON object of header name to value keeps
-- that a configuration decision in /admin/upstreams instead of a code change
-- per provider: the gateway applies the object to every request it sends to
-- this backend (probe, identification, test, traffic).
ALTER TABLE backends ADD COLUMN extra_headers TEXT NOT NULL DEFAULT '{}';
