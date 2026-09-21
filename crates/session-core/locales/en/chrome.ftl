# Strings owned by `session-core/src/chrome.rs` — theme + language
# toggles, generic toast/flash chrome shared by every page.

chrome-theme-toggle-title = Toggle theme
chrome-theme-toggle-aria-to-light = Switch to light theme
chrome-theme-toggle-aria-to-dark = Switch to dark theme
chrome-lang-switcher-aria = Choose language

# Web Push turn-complete notifications (server-sent body; `spawn_assistant_worker`).
push-untitled-conversation = New conversation
push-turn-complete-body = Your answer is ready.
push-turn-error-body = The turn ended with an error.

# Web Push: a connector's authorization died (proactive-refresh sweep,
# `tools::mcp::worker`). { $connector } is the connector's display name.
push-connector-reconnect-title = Connection needs your sign-in
push-connector-reconnect-body = { $connector } was disconnected — open Integrations to reconnect.

# SPA-only: the root route, which only routes on to /chat.
chrome-opening-conversations = Opening your conversations…

# SPA-only: /login, which exists to bounce straight to the IdP.

searchable-select-search-placeholder = Search options…
searchable-select-search-aria = Search { $field }
searchable-select-clear-search = Clear search
searchable-select-no-results = No matching options.
searchable-select-model-gdpr = GDPR
searchable-select-model-nda = NDA

# Shown in place of a page whose optional feature the operator has switched
# off at /admin/settings. The nav entry is gone in that case; this answers
# someone who followed an old link or typed the URL.
feature-disabled-body = { $feature } is turned off for this gateway, so this page has nothing to show. An administrator can switch it on under Settings.
feature-disabled-settings-link = Open settings

multi-select-none = None selected
multi-select-count = { $count } selected
multi-select-clear = Clear all
multi-select-unknown = Not registered here — remove it or create it
multi-select-wildcard-tools = Every tool, including ones added later
multi-select-wildcard-skills = Every skill, including ones added later
multi-select-shadowed = Covered by ✱
multi-select-wildcard-warning = ✱ also grants whatever tools a future release adds, without review. On a group that is not an admin group, prefer listing the tools you mean.
