# Strings owned by `gateway/src/rama_server/pages/tokens.rs` — the
# /tokens page: token list, create form, per-token tool/MCP controls,
# minted-secret banner, and the account summary card at the foot of
# the page.

tokens-page-heading = API tokens
tokens-intro = Bearer tokens for the OpenAI-compatible API. The plaintext is shown only at creation time — store it somewhere safe.

tokens-create-heading = Create token
tokens-name-label = Name
tokens-name-placeholder = e.g. laptop, ci-runner
tokens-ttl-label = TTL (days)
tokens-create-submit = Create token

tokens-list-heading = Your tokens
tokens-list-empty = No tokens yet. Create one above.

tokens-badge-revoked = revoked
tokens-badge-active = active
tokens-remove-button = Remove
tokens-rotate-button = Rotate
tokens-rotate-title = Issue a new secret for this token (keeps its name and settings)
tokens-revoke-button = Revoke

tokens-row-meta = created { $created } · last used { $last_used } · expires { $expires }
tokens-last-used-never = never

tokens-tool-use-aria = Tool use
tokens-tool-use-label = Tool use

tokens-mcp-allow-description = Approval-required connector tools can't prompt over the API; enabling runs them without asking.

tokens-minted-heading = Token created
tokens-minted-copy-warning = Copy the value now — you won't be able to see it again.
tokens-copy-aria = Copy token
tokens-minted-name = Name: { $name }

tokens-account-user-id-label = User ID

tokens-mcp-ask-enabled-toast = Ask-mode MCP tools over API enabled for this token.
tokens-mcp-ask-disabled-toast = Ask-mode MCP tools over API disabled for this token.

# Web Push "turn complete" opt-in card (rendered by `render_push_card`; wired
# client-side by `ui/ts/push.ts`). Device-local notification settings.
tokens-push-enable = Enable on this device
tokens-push-disable = Disable on this device
tokens-push-on = Notifications are on for this device.
tokens-push-enabled = Notifications enabled on this device.
tokens-push-disabled = Notifications disabled on this device.
tokens-push-error = Could not change notification settings.

# Per-token usage, model allowlist, and quota (the /tokens row panels).
tokens-usage-line = this month: { $requests } requests · { $tokens } tokens · { $cost }
tokens-models-summary-all = Models: all
tokens-models-summary-restricted = Models: { $count } selected
tokens-models-help = Off, this token follows your own access, including models added later. On, it may use only the models you tick — a model added to the gateway afterwards stays blocked until you tick it here too.
tokens-models-restrict-label = Limit this token to specific models
tokens-models-save = Save models
tokens-models-saved-toast = Token limited to { $count } models.
tokens-models-cleared-toast = Token can use every model you can.
tokens-limits-add = Add quota
tokens-limits-saved-toast = Token quota saved.

# SPA-only: the Svelte /tokens row panels and their confirm prompts.
tokens-quota-max-placeholder = max
tokens-revoke-confirm = Revoke this token? Clients using it stop working immediately.
tokens-rotate-confirm = Issue a new secret? The old one stops working immediately.
tokens-remove-confirm = Delete this token row for good?

tokens-create-description = Mint a new bearer token for the OpenAI-compatible API.
tokens-tool-use-description = Let this token call gateway tools (web search, RAG, …).
tokens-capabilities-summary = Capabilities
tokens-mcp-allow-aria = Allow ask-mode MCP tools over API
tokens-mcp-allow-label = Allow “ask” MCP tools over API
tokens-account-heading = Account
tokens-signed-in-as = Signed in as { $email }
tokens-account-oidc-label = OIDC roles
tokens-account-rbac-label = RBAC role IDs
tokens-roles-none = none
tokens-roles-none-granted = none granted
tokens-push-heading = Notifications
tokens-push-description = Get a notification on this device when an assistant turn you started finishes while you're away from the app.
tokens-push-off = Notifications are off for this device.
tokens-push-denied = This browser has blocked notifications. Allow them in your browser settings to enable.
tokens-push-unsupported = This browser doesn't support notifications.
tokens-models-none-picked = Tick at least one model, or turn the limit off.
tokens-limits-summary-none = Quota: none
tokens-limits-summary-some = Quota: { $count } rule(s)
tokens-limits-help = A cap on this token alone. Your own budget still applies, so this can only narrow what the token may spend, never widen it.
tokens-limits-remove = Remove
tokens-limits-removed-toast = Token quota removed.
tokens-limits-admin-badge = set by admin
tokens-models-admin-set = An operator also restricts this token to: { $models }. Your own selection narrows that further; it cannot widen it.
