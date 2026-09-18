-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright (C) 2026 croit GmbH
--
-- Per-backend maintenance switch.
--
-- Taking a backend out of rotation for maintenance had no first-class way to
-- happen: the choices were to delete it (losing its key, models and aliases),
-- to point it somewhere invalid, or to let it fail health checks and hope the
-- picker noticed. All three are worse than a switch.
--
-- `enabled = 0` means "do not route here", and nothing else. The row keeps
-- every setting, the health probe keeps running (so the admin page still shows
-- whether the box is back), and — this is the point — the models it serves stay
-- *known* to the gateway. A request for one of them therefore waits for, or is
-- told about, a temporary outage (503/529) instead of being told the model does
-- not exist (404), and any sibling backend in the pool simply takes the load.
--
-- Default 1 so every existing backend keeps serving across the migration.

ALTER TABLE backends ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;
