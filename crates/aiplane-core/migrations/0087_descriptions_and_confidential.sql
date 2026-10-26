-- Operator-facing model descriptions (the chat picker's subtext), for model
-- ids, backend aliases and automatic-route aliases, plus the sticky
-- "confidential" mark on a conversation. A confidential conversation may only
-- be routed to models whose pools carry the compliance the operator requires;
-- the flag never clears once set.
ALTER TABLE model_defaults ADD COLUMN description TEXT NOT NULL DEFAULT '';

ALTER TABLE automatic_routes ADD COLUMN description TEXT NOT NULL DEFAULT '';

ALTER TABLE chat_session_settings ADD COLUMN confidential INTEGER NOT NULL DEFAULT 0;
