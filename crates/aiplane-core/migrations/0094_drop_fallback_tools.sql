-- The tools fallback was stored by the admin model editor but never read at
-- runtime: a model whose Tools capability is off is offered no tools at all,
-- and only vision has a fallback model. The column and its dead values go.
ALTER TABLE model_defaults DROP COLUMN fallback_tools;
