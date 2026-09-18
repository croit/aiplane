-- Let a collection re-sync itself on a schedule.
--
-- Until now a corpus was refreshed by an operator clicking Re-index or by
-- something outside the gateway ringing `/hooks/rag/{token}`. That covers a
-- file host with a webhook app; it covers nothing that only knows how to be
-- polled. A mailing-list archive is the case that made this necessary — there
-- is no doorbell to press, so "index it daily" had to mean an external cron
-- line or nothing.
--
-- Minutes rather than a cron expression: the question a collection answers is
-- "how stale may this be", not "when exactly". Zero means never, which is
-- what every existing row gets — this migration changes no behaviour on its
-- own.
ALTER TABLE rag_collections
    ADD COLUMN refresh_interval_mins INTEGER NOT NULL DEFAULT 0;
