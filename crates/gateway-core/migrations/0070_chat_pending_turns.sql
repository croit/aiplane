-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright (C) 2026 croit GmbH
--
-- Messages that were sent but have not started being answered yet.
--
-- Until now this queue lived in the browser's `localStorage`: the user pressed
-- enter, and whether that message existed at all was something only their tab
-- knew. Close it and the message was gone; open the same conversation on a
-- phone and it was not there; a colleague reading a shared conversation saw
-- nothing. "Sent" has to be a fact about the conversation, not about a page
-- that happens to still be open.
--
-- The message itself is an ordinary `chat_turns` row, written the moment it is
-- accepted — it *is* a user turn, and the transcript should read that way.
-- This table holds only what a turn needs in order to be *started* later, and
-- the row is deleted the moment the worker takes it. So it is a work queue,
-- not a second copy of the conversation: nothing here is user-visible content,
-- and losing a row would cost a turn its start parameters, never its text.
--
-- Why not a fifth `chat_turns.status` instead: that column is interpreted in
-- 66 places across six crates — export, the FTS triggers, compaction, webhooks,
-- history replay, the startup sweep. A new variant would have to be right in
-- all of them, and wrong in any one of them is a silent bug. "A user turn with
-- no answer after it" needs no new state at all.
--
-- `client_ip` and `secure` are carried because the request that accepted the
-- message is long gone by the time the turn runs, and the request-context
-- system message (and `get_user_location`) are built from them. They describe
-- the moment the user hit enter, which is the honest answer to "where was this
-- asked from".

CREATE TABLE chat_pending_turns (
    turn_id     TEXT PRIMARY KEY NOT NULL,   -- the user turn waiting for an answer
    session_id  TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    model       TEXT NOT NULL,               -- what to answer with
    voice       INTEGER NOT NULL DEFAULT 0,  -- submitted from voice mode
    client_ip   TEXT,                        -- as seen when it was accepted
    secure      INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    FOREIGN KEY (turn_id) REFERENCES chat_turns(id) ON DELETE CASCADE,
    FOREIGN KEY (session_id) REFERENCES chat_sessions(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- The scheduler's read: this user's oldest waiting turn, and whether a given
-- conversation has one at all.
CREATE INDEX chat_pending_turns_user_created ON chat_pending_turns(user_id, created_at);
CREATE INDEX chat_pending_turns_session ON chat_pending_turns(session_id, created_at);
