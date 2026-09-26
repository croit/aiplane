-- Existing token preferences wrote an enabled row for every visible tool.
-- With three states, those implicit allows must return to Auto so upgrading
-- does not advertise a large catalog to every token with tool use enabled.
DELETE FROM token_tool_prefs WHERE enabled = 1;
