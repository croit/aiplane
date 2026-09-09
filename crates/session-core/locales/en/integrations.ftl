# Strings owned by `gateway/src/rama_server/pages/integrations.rs` — the
# per-user `/integrations` connector store: connect/disconnect flow,
# OAuth error copy, and per-tool permission controls.

integrations-heading = Integrations
integrations-intro = Connect your own accounts so the assistant can act on your behalf — reading your email, calendar, files, repositories, and more. Each connection uses your own permissions and can be disconnected anytime.
integrations-empty = No connectors are available yet. An administrator can enable them under Admin → Connectors.

integrations-badge-connected = Connected

integrations-disconnect-button = Disconnect
integrations-disconnect-confirm = Disconnect this integration? Your stored access token will be deleted.
integrations-connect-button = Connect

integrations-token-label = Your API token
integrations-token-placeholder = paste your token

integrations-error-unknown-connector = unknown or disabled connector
integrations-error-forbidden-role = you don't have access to this connector
integrations-error-not-oauth = this connector does not use OAuth
integrations-error-oauth-discovery-failed = OAuth discovery failed: { $error }
integrations-error-needs-setup-no-client = this connector needs setup: no client id is configured and the provider offers no dynamic registration. Ask an admin to add an OAuth client.
integrations-error-sealing-client-secret = sealing client secret: { $error }
integrations-error-dcr-failed = dynamic client registration failed: { $error }
integrations-error-needs-setup-admin = this connector needs setup: an admin must configure an OAuth client id.
integrations-error-building-authorize-url = building authorize URL: { $error }
integrations-error-persisting-authorization = persisting authorization: { $error }
integrations-error-provider-error = provider returned an error: { $error } { $desc }
integrations-error-callback-missing = callback missing code or state
integrations-error-auth-expired = this authorization has expired or was already used — start again from Integrations
integrations-error-loading-authorization = loading authorization: { $error }
integrations-error-state-mismatch = authorization state did not match your session
integrations-error-connector-missing = the connector no longer exists
integrations-error-decrypting-client-secret = decrypting client secret: { $error }
integrations-error-connector-missing-client-id = connector is missing its OAuth client id
integrations-error-sealing-access-token = sealing access token: { $error }
integrations-error-sealing-refresh-token = sealing refresh token: { $error }
integrations-error-saving-connection = saving connection: { $error }

# SPA-only: the Svelte /integrations connector list.
integrations-badge-not-connected = Not connected
integrations-toast-connected = Connected { $name }.
