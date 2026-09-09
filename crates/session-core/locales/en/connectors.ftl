# Strings owned by `gateway/src/rama_server/pages/connectors.rs` — the
# admin `/admin/connectors` page that curates the MCP connector catalog
# the per-user `/integrations` store draws from.
#
# A handful of keys are split into short fragments (e.g.
# `connectors-oauth-help-gws-1` / `-2` / `-3` …) because the source HTML
# splices plain text around inline `<strong>` / `<code>` / `<a>` tags.
# Fluent trims leading/trailing whitespace from a message value, so the
# glue space between a fragment and its neighbouring tag is a literal
# `" "` in the Rust call site, not part of the translated string —
# translators should not add leading/trailing spaces to these values.

connectors-heading = Connectors
connectors-restore-defaults-button = Restore defaults

connectors-badge-enabled = Enabled
connectors-badge-disabled = Disabled
connectors-disable-button = Disable
connectors-enable-button = Enable
connectors-delete-confirm = Delete this connector? It is removed for all users, along with their stored connections and tokens. This cannot be undone.
connectors-delete-button = Delete

connectors-field-key-readonly-label = Key
connectors-field-name-label = Name
connectors-field-url-label = MCP server URL
connectors-field-auth-label = Authentication
connectors-auth-option-oauth = OAuth 2.1 (each user authorizes via the provider)
connectors-auth-option-token = User-supplied token (each user pastes their own API token)
connectors-auth-option-none = None (public server, no authentication)
connectors-field-allowed-groups-label = Allowed groups (comma-separated)
connectors-save-changes-button = Save changes
connectors-add-connector-button = Add connector

# SPA connector form: the compact comma-separated scope field, the combined
# secret/token field, and the restore-defaults confirmation.
connectors-restore-defaults-confirm = Re-seed the built-in catalog entries?
connectors-field-scopes-csv-label = Scopes (comma-separated)
connectors-field-secret-or-token-label = Client secret / token
