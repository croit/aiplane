-- A transient token-refresh failure no longer takes a connection out of
-- service. Connections it parked go back to work; a dead one becomes
-- 'reauth' on its next refresh.
UPDATE mcp_connections SET status = 'connected' WHERE status = 'error';
