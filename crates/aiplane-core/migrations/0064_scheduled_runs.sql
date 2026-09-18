-- Per-fire history for scheduled actions (0021), mirroring `webhook_runs`
-- (0038). Until now a schedule kept only `last_*`: the single most recent
-- fire. Every earlier run's chat was still in the database, but nothing
-- connected it to the schedule that opened it, so the page could offer one
-- link and no way back to the conversations before it.
--
-- One row per fire. `session_id` is the chat the run opened — that is the
-- link the user comes to /scheduled for. A schedule with
-- `reuse_conversation = 1` points every run at the same session by design;
-- the run list still shows each fire separately, because *when it ran* and
-- *whether it worked* differ per run even when the conversation does not.
--
-- `status` is NULL only for the window between the run starting and its
-- outcome landing; the worker always writes one of 'ok' | 'error' when the
-- run returns, and a process killed mid-run leaves the row pending forever
-- (the same accepted gap webhook_runs has).
--
-- The `scheduled_actions.last_*` columns stay: they are the denormalized
-- summary the list row reads without touching this table.
CREATE TABLE scheduled_runs (
    id         TEXT PRIMARY KEY NOT NULL,   -- UUID v4
    action_id  TEXT NOT NULL,               -- owning scheduled action
    fired_at   TEXT NOT NULL,               -- RFC 3339, when the run started
    status     TEXT,                        -- NULL (in flight) | 'ok' | 'error'
    session_id TEXT,                        -- the chat session this run opened
    error      TEXT,                        -- error detail when status = 'error'
    created_at TEXT NOT NULL,               -- RFC 3339
    FOREIGN KEY (action_id) REFERENCES scheduled_actions(id) ON DELETE CASCADE
);

-- The runs list reads one action's history newest-first.
CREATE INDEX scheduled_runs_by_action ON scheduled_runs(action_id, fired_at DESC);

-- Backfill the one run each existing schedule remembers, so an action that
-- has already fired doesn't present an empty history on the new page. Runs
-- older than the most recent one were never recorded and cannot be
-- recovered. The id is derived from the action id rather than random so the
-- migration is deterministic and the row is recognizable as a backfill.
INSERT INTO scheduled_runs (id, action_id, fired_at, status, session_id, error, created_at)
SELECT 'legacy-' || id, id, last_run_at, last_status, last_session_id, last_error, last_run_at
FROM scheduled_actions
WHERE last_run_at IS NOT NULL;
