CREATE TABLE personal_dot_sessions (
    session_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    dot_id TEXT NOT NULL REFERENCES personal_dots(principal_id) ON DELETE CASCADE,
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX idx_personal_dot_sessions_owner_dot
    ON personal_dot_sessions (owner_user_id, dot_id, created_at DESC);
