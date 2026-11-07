-- A transient token-refresh failure no longer takes a connection out of
-- service. Connections it parked go back to work; a dead one becomes
-- 'reauth' on its next refresh.
UPDATE mcp_connections SET status = 'connected' WHERE status = 'error';

-- When the MCP server began rejecting the connection's credential, and how
-- often it has since, both cleared by the next accepted connect. Repeated
-- rejections over an hour mean 'reauth'.
ALTER TABLE mcp_connections ADD COLUMN rejected_since TEXT;
ALTER TABLE mcp_connections ADD COLUMN rejections INTEGER NOT NULL DEFAULT 0;
