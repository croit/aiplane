-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright (C) 2026 croit GmbH
--
-- Mid-turn interjections: what the user typed *while* an answer was being
-- written, meant for that answer rather than the next one.
--
-- Until now the composer was simply dead while a turn streamed, so the only
-- way to add the context the model was visibly missing was to throw the whole
-- turn away and start over. An interjection is folded into the running turn at
-- the next tool round, as one more user message in the prompt.
--
-- Why these are rows and not just an in-memory queue on the worker handle:
-- the model acts on them. A note that changed the answer but that the
-- transcript cannot account for is the same class of bug as a dropped
-- parameter — the conversation would read as if the model had invented a
-- constraint. They are also what the replayed history hands the model on the
-- *next* turn, so they have to outlive the process that delivered them.
--
-- Why a side table rather than another `chat_turns` row: a turn's seq is
-- already taken by the assistant row being written, and `UNIQUE (session_id,
-- seq)` leaves no room between two turns. The interjection genuinely belongs
-- *inside* one turn, which is exactly what a side table keyed on the turn
-- expresses — the same shape, and for the same reason, as `chat_tool_calls`.
--
-- `status` is the honesty column, and it has four values because there are
-- four genuinely different fates:
--
--   pending    typed, not yet accounted for. The turn is still running.
--   delivered  the model was handed it mid-turn, at a round boundary.
--   resent     the turn ended before any round could carry it, so it was
--              submitted as an ordinary next message instead.
--   discarded  the turn ended before reaching it and the user threw it away
--              rather than re-sending it.
--
-- Collapsing `resent` into `delivered` would make the transcript claim the
-- running answer took the note into account when it demonstrably could not.
-- Collapsing it into `pending` would leave a note that was in fact acted on
-- looking like one that was dropped.
--
-- The column also does duty as a claim check. Settling is
-- `WHERE status = 'pending'`, so two browser tabs looking at the same
-- finished turn cannot both re-send the same interjection: the second one's
-- UPDATE matches no row and its submit is refused.

CREATE TABLE chat_turn_steers (
    id          TEXT PRIMARY KEY NOT NULL,   -- UUID
    turn_id     TEXT NOT NULL,               -- the assistant turn being steered
    seq         INTEGER NOT NULL,            -- order within turn, 0-based
    text        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',  -- pending|delivered|resent|discarded
    created_at  TEXT NOT NULL,
    settled_at  TEXT,                        -- when it stopped being pending
    FOREIGN KEY (turn_id) REFERENCES chat_turns(id) ON DELETE CASCADE,
    UNIQUE (turn_id, seq)
);

CREATE INDEX chat_turn_steers_turn_seq ON chat_turn_steers(turn_id, seq);
