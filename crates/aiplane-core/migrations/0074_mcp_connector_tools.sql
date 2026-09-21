-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright (C) 2026 croit GmbH

-- What tools each MCP connector actually exposes, cached so the admin UI can
-- offer them.
--
-- Rationale: an MCP server's tool list exists only on the server. It is
-- discovered when `McpManager::ensure` connects, which happens on a *user's*
-- behalf with that user's credentials — an admin editing a group has no way to
-- ask. Without this table a per-tool grant could only be typed by hand, which is
-- the silent-typo failure the picker work (#45) set out to remove.
--
-- Deliberately a cache, not a source of truth:
--   * it is written opportunistically after a successful connection, so a
--     connector nobody has ever connected has no rows and the editor must say so
--     rather than render an empty list as "no tools";
--   * a row going stale is harmless. The grant is matched against the live tool
--     id at call time, so a tool that disappeared simply never matches, and one
--     that appeared is reachable via the `mcp__<server>` family grant.
-- Nothing reads it on the request path.
CREATE TABLE mcp_connector_tools (
    -- The connector this tool belongs to (`mcp_catalog_connectors.key`).
    connector_key TEXT NOT NULL REFERENCES mcp_catalog_connectors(key) ON DELETE CASCADE,
    -- The gateway-side tool id (`mcp__<server>__<tool>`), i.e. exactly the
    -- string a grant names — stored whole so the admin surface never has to
    -- rebuild it and risk disagreeing with `mcp::tool_id`.
    tool_id       TEXT NOT NULL,
    -- The server's own description, for the picker row. Empty when it gave none.
    description   TEXT NOT NULL DEFAULT '',
    seen_at       TEXT NOT NULL,
    PRIMARY KEY (connector_key, tool_id)
) STRICT;
