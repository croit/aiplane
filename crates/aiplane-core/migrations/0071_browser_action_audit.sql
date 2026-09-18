-- Append-only audit of `browser_control` batches.
--
-- The tool acts in a user's own browser, with their sessions, on sites the
-- gateway never sees. When something is later found to have been clicked or
-- submitted, this table is the only place that records who asked for it and
-- what the outcome was — the browser itself keeps no trail an operator can
-- read, and the extension's activity list lives on one machine.
--
-- Shaped like `mcp_tool_audit` (migration 0036): no foreign keys and the
-- email denormalised on, so the trail stays readable after a user is deleted
-- and cannot be erased by a cascade.
--
-- What is deliberately NOT stored: page content, typed text, screenshots. The
-- point of the row is accountability for an action, and a table that also held
-- what was typed into a login form would be a worse liability than the problem
-- it documents. `actions` is the list of step names, nothing else.

CREATE TABLE browser_action_audit (
    id          TEXT PRIMARY KEY NOT NULL,
    user_id     TEXT NOT NULL,
    user_email  TEXT NOT NULL,          -- denormalised; survives user deletion
    turn_id     TEXT NOT NULL,          -- assistant turn that asked
    session_id  TEXT,                   -- chat session context, when present
    actions     TEXT NOT NULL,          -- comma-separated step names, no payloads
    writes      INTEGER NOT NULL,       -- how many of them change a page
    outcome     TEXT NOT NULL,          -- 'ok' | 'partial' | 'refused' | 'no_extension' | 'timeout'
    detail      TEXT,                   -- error or refusal reason
    created_at  TEXT NOT NULL
) STRICT;

CREATE INDEX idx_browser_action_audit_created ON browser_action_audit (created_at DESC);
CREATE INDEX idx_browser_action_audit_user ON browser_action_audit (user_id, created_at DESC);
