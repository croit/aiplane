# Strings owned by `gateway/src/rama_server/pages/tokens.rs` — the
# /tokens page: token list, create form, per-token tool/MCP controls,
# minted-secret banner, and the account summary card at the foot of
# the page.

tokens-page-heading = API tokens
tokens-intro = Tokens for apps and code that connect to AIplane. The full token is shown only when you create or rotate it — copy it and keep it private.

tokens-create-heading = Create token
tokens-name-label = Name
tokens-name-placeholder = e.g. laptop, ci-runner
tokens-ttl-label = TTL (days)
tokens-create-submit = Create token

tokens-list-heading = Your tokens
tokens-list-empty = No tokens yet. Create one with the button above.

tokens-badge-revoked = revoked
tokens-badge-active = active
tokens-remove-button = Remove
tokens-rotate-button = Rotate
tokens-rotate-title = Issue a new secret for this token (keeps its name and settings)
tokens-revoke-button = Revoke

tokens-row-meta = created { $created } · last used { $last_used } · expires { $expires }
tokens-last-used-never = never

tokens-tool-use-label = Tool use

tokens-mcp-allow-description = Approval-required connector tools can't prompt over the API; enabling runs them without asking.

tokens-minted-heading = Token created
tokens-minted-copy-warning = Copy the value now — you won't be able to see it again.
tokens-copy-aria = Copy token
tokens-minted-name = Name: { $name }

tokens-account-user-id-label = User ID

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
tokens-models-summary-restricted = Models: { $count } selected
tokens-models-help = Off, this token follows your own access, including models added later. On, it may use only the models you tick — a model added to the gateway afterwards stays blocked until you tick it here too.
tokens-models-restrict-label = Limit this token to specific models
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
tokens-panel-close = Close
tokens-edit-button = Edit
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
tokens-limits-help = A cap on this token alone. Your own budget still applies, so this can only narrow what the token may spend, never widen it.
tokens-limits-remove = Remove
tokens-limits-admin-badge = set by admin
tokens-models-admin-set = An operator also restricts this token to: { $models }. Your own selection narrows that further; it cannot widen it.

# Client setup guides and token-page tabs.
tokens-tab-tokens = Tokens
tokens-tab-guides = Setup guides
tokens-tab-account = Account
tokens-guides-heading = Set up a client
tokens-guides-intro = Choose an app below and follow the steps. You can use the same token in more than one app, but a separate token per app is easier to manage.
tokens-guides-before = First, create a token in the Tokens tab and copy it. The full token is shown only once. Keep it private.
tokens-guide-opencode = OpenCode
tokens-guide-claude = Claude Code
tokens-guide-python = Python (OpenAI)
tokens-opencode-step-1 = Create a token in the Tokens tab and copy it.
tokens-opencode-step-2 = In OpenCode, run /connect, choose Other, enter aiplane as the provider ID, then paste your token.
tokens-opencode-step-3 = Save this configuration in ~/.config/opencode/opencode.jsonc. Replace YOUR_MODEL_ID with a chat model ID you can use.
tokens-opencode-finish = Run opencode, then use /models to choose aiplane/YOUR_MODEL_ID.
tokens-claude-step-1 = Create a token in the Tokens tab and copy it.
tokens-claude-step-2 = Choose a chat model ID you can use. Your administrator may have set up an alias for Claude Code.
tokens-claude-step-3 = Paste these commands into your terminal. Replace the token and model ID first.
tokens-claude-finish = Claude Code uses the base URL without /v1. If you get a model-not-found error, ask your administrator which alias to use.
tokens-python-step-1 = Create a token in the Tokens tab and copy it.
tokens-python-step-2 = Install the OpenAI Python package with python -m pip install openai, then set OPENAI_API_KEY to your token in your terminal.
tokens-python-step-3 = Save this example as chat.py. Replace YOUR_MODEL_ID with a chat model ID you can use.
tokens-python-finish = Run python chat.py. Keep the token in an environment variable, not in the script.
tokens-guide-omp = Oh My Pi
tokens-guide-pi = Pi
tokens-omp-finish = Run omp, then use /model to choose aiplane/YOUR_MODEL_ID.
tokens-omp-step-1 = Create a token in the Tokens tab and copy it.
tokens-omp-step-2 = Save this configuration in ~/.omp/agent/models.yml. Replace the token and YOUR_MODEL_ID first, and set contextWindow to the model's context size.
tokens-pi-finish = Run pi, then use /model to choose aiplane/YOUR_MODEL_ID.
tokens-pi-step-1 = Create a token in the Tokens tab and copy it.
tokens-pi-step-2 = Save this configuration in ~/.pi/agent/models.json. Replace the token and YOUR_MODEL_ID first.
tokens-guides-model-note = Need a model ID? Your available models are shown in the chat model picker or by GET /v1/models with your token.

notifications-loading = Loading notification settings…
notifications-unavailable = Notifications are unavailable on this gateway. Ask an administrator to check Push settings.
notifications-admin-settings-link = Open Push settings
tokens-tile-models = Models
tokens-tile-tools = Tools
tokens-tile-budget = Budget
tokens-save = Save
tokens-menu-aria = Token actions
tokens-expires-today = expires today
tokens-expires-soon = { $days ->
    [one] expires in { $days } day
   *[other] expires in { $days } days
}
tokens-models-tile-all = All { $count } models — follows your access
tokens-models-tile-some = { $count } of { $total } models
tokens-models-tile-compliant = All GDPR compliant and NDA covered
tokens-models-tile-noncompliant = { $count ->
    [one] { $count } model without GDPR or NDA cover
   *[other] { $count } models without GDPR or NDA cover
}
tokens-models-tile-admin = An operator allows { $count } of them
tokens-models-max-price = up to { $price } per 1M output tokens
tokens-models-noncompliant-warning = { $count ->
    [one] { $count } model here is not GDPR compliant or not NDA covered. Don’t send personal data or confidential material through this token.
   *[other] { $count } models here are not GDPR compliant or not NDA covered. Don’t send personal data or confidential material through this token.
}
tokens-models-search = Search models
tokens-models-filter-gdpr = GDPR compliant
tokens-models-filter-nda = NDA covered
tokens-models-filter-free = No charge
tokens-models-select-compliant = Select all GDPR + NDA
tokens-models-select-none = Clear selection
tokens-models-empty = No model matches.
tokens-models-alias = alias of { $target }
tokens-models-gdpr-ok = GDPR compliant: personal data stays protected.
tokens-models-nda-ok = Covered by a confidentiality agreement.
tokens-models-price-free = no charge
tokens-models-price-tokens = { $input } in / { $output } out per 1M
tokens-models-price-per-images = { $price } per image
tokens-models-price-per-characters = { $price } per character
tokens-models-price-per-seconds = { $price } per second
tokens-models-kind-chat = Chat
tokens-models-kind-transcription = Speech to text
tokens-models-kind-speech = Text to speech
tokens-models-kind-embedding = Embedding
tokens-models-kind-rerank = Reranking
tokens-models-kind-image = Images
tokens-models-kind-system_one = System One
tokens-tools-tile-on = Allowed
tokens-tools-tile-off = Off
tokens-tools-tile-pinned = { $count } always on
tokens-tools-tile-mcp-allowed = “Ask” MCP tools run without asking
tokens-tools-tile-mcp-blocked = “Ask” MCP tools blocked
tokens-tools-capabilities-help = Which tools and skills this token may use.
tokens-tools-saved-toast = Token tools saved.
tokens-budget-tile-none = No limit of its own
tokens-budget-tile-more = +{ $count } more
tokens-budget-owner-applies = Your own budget applies as well
tokens-budget-token-heading = This token
tokens-budget-owner-heading = Applies as well: your own budget
tokens-budget-owner-none = You have no budget limit.
tokens-budget-dimension = What to limit
tokens-budget-window = Period
