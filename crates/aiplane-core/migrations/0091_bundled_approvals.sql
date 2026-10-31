-- One approval for every call of a round that asks for one: the waiting
-- calls after the first, as a JSON array of pending calls.
ALTER TABLE chat_turn_suspensions ADD COLUMN bundled_calls TEXT NOT NULL DEFAULT '[]';

-- Calls approved for the rest of a conversation, by tool and the exact
-- actions asked about ('' when the tool declares none).
CREATE TABLE chat_session_tool_approvals (
    session_id  TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    tool        TEXT NOT NULL,
    actions     TEXT NOT NULL,
    approved_at TEXT NOT NULL,
    PRIMARY KEY (session_id, tool, actions)
) STRICT;
