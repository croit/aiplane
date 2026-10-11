CREATE TABLE personal_dot_sessions (
    session_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    dot_id TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX idx_personal_dot_sessions_owner_dot
    ON personal_dot_sessions (owner_user_id, dot_id, created_at DESC);

ALTER TABLE scheduled_actions ADD COLUMN dot_id TEXT;
ALTER TABLE webhooks ADD COLUMN dot_id TEXT;

ALTER TABLE scheduled_runs ADD COLUMN assistant_turn_id TEXT;
ALTER TABLE scheduled_runs ADD COLUMN tools_enabled INTEGER NOT NULL DEFAULT 0
    CHECK (tools_enabled IN (0, 1));
ALTER TABLE scheduled_runs ADD COLUMN history_limit INTEGER
    CHECK (history_limit IS NULL OR history_limit >= 0);
CREATE UNIQUE INDEX scheduled_runs_assistant_turn ON scheduled_runs (assistant_turn_id)
    WHERE assistant_turn_id IS NOT NULL;

ALTER TABLE webhook_runs ADD COLUMN assistant_turn_id TEXT;
ALTER TABLE webhook_runs ADD COLUMN tools_enabled INTEGER NOT NULL DEFAULT 0
    CHECK (tools_enabled IN (0, 1));
ALTER TABLE webhook_runs ADD COLUMN history_limit INTEGER
    CHECK (history_limit IS NULL OR history_limit >= 0);
CREATE UNIQUE INDEX webhook_runs_assistant_turn ON webhook_runs (assistant_turn_id)
    WHERE assistant_turn_id IS NOT NULL;
