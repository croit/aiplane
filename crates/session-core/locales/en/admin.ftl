# Strings owned by `gateway/src/rama_server/pages/admin.rs` — the
# `/admin/models` page: default-model pickers plus one filterable list of every
# advertised model, each with a single consolidated editor (pricing, context
# window, reasoning style + budgets/efforts, capabilities, sampling defaults).

admin-heading = Models

# List column headers.
admin-col-model = Model

# Collapsed-row values.
admin-not-configured = not configured

# "Configured" facet badges.
admin-badge-ctx = CTX

# Editor.
admin-save-model = Save model
admin-clear-overrides = Clear all overrides
admin-cancel = Cancel

admin-toml-defaults-label = Sampling defaults (TOML)

# Per-model pricing for cost accounting (price per 1M tokens, input / output).
admin-price-in-label = Price in
admin-price-out-label = Price out
admin-price-in-placeholder = unpriced
admin-price-out-placeholder = unpriced

# Context window (drives auto-compaction).
admin-context-window-full-label = Context window (tokens)
admin-context-window-placeholder = default

# Per-feature default models (the model pre-selected in the chat/voice
# pickers, and the API fallback when a call omits a model).
admin-defaults-heading = Default models

# Model capabilities (tri-state) + fallback model refs.
admin-cap-tools = Tools

# Web search backend (`search_web` tool). Formerly the SEARCH_PROVIDER /
# SEARXNG_URL / BRAVE_SEARCH_API_KEY environment variables.
admin-search-heading = Web search
admin-search-provider-label = Provider
admin-search-provider-searxng = SearXNG (self-hosted)
admin-search-provider-brave = Brave Search API
admin-search-searxng-url-label = SearXNG base URL
admin-search-searxng-url-placeholder = https://searxng.example.com
admin-search-brave-key-label = Brave API key
admin-search-brave-key-placeholder = leave blank to keep the current key
admin-search-save = Save web search
admin-search-saved = web-search settings saved

# ─── SvelteKit admin SPA ─────────────────────────────────────────────────────
# The admin shell's role guard, the /admin/comfyui workflow catalog, and the
# bits of the /admin/models editor the SPA renders that the legacy page didn't.

admin-needs-admin-role = These pages need the admin role.
admin-overwrite-existing = overwrite existing
admin-comfyui-reload = Reload catalog
admin-comfyui-reloaded = Reloaded { $count } workflow(s).
admin-comfyui-empty = No workflows loaded — check the content directory.
admin-defaults-model-aria = Default model for { $feature }
admin-defaults-set = Set
admin-search-provider-none = None
admin-add-overrides-heading = Add model overrides
admin-edit-model-heading = Edit { $model }
admin-add-model = Add…
admin-pricing-unit-label = Pricing unit
admin-pricing-unit-mtok = per Mtok
admin-pricing-unit-ktok = per Ktok
admin-pricing-unit-kimgs = per 1k images
admin-clear-overrides-confirm = Drop all stored overrides for { $model }?
admin-users-col-email = Email
