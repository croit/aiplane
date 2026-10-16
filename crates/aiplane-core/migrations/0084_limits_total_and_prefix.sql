-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright (C) 2026 croit GmbH

-- Most-specific-wins for model scopes + total (cumulative) flag.
--
-- So far an aggregate rule (`model` NULL) and a model-scoped rule on the
-- same (subject, dimension, window) were *both* enforced, so the aggregate
-- always capped the specific rule and a per-model allowance could never
-- exceed it. From here a model-scoped rule overrides the aggregate in its
-- (dimension, window): for a call on model M only the most specific
-- matching override counts. A rule marked `is_total = 1` keeps the old
-- behaviour and is checked additionally (a real overall ceiling).
--
-- `model` may now also hold a prefix scope with a single trailing `*`
-- (e.g. 'claude-*'), matching every model id with that prefix. Exact scopes
-- outrank prefix scopes, which outrank the aggregate. Existing rows keep
-- their exact-match meaning and become totals, preserving enforcement until
-- an operator reconfigures them.

ALTER TABLE limits ADD COLUMN is_total INTEGER NOT NULL DEFAULT 0;

UPDATE limits SET is_total = 1;
