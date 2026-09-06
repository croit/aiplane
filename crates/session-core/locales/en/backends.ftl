# Strings owned by `gateway/src/rama_server/pages/backends.rs` — the
# read-only `/admin/backends` operator view of the upstream pools.

backends-page-title = Upstream backends — LLM Gateway
backends-heading = Upstream backends
backends-description-prefix = Live view of the configured upstream pools — health, in-flight load against each backend's cap, and the models each one currently advertises. Read-only: routing is driven entirely by what the backends report on their
backends-description-suffix = probe.
backends-summary = { $total } backends · { $healthy } healthy · { $down } down
backends-unknown-fallback-prefix = Unknown-model fallback —
backends-empty-prefix = No upstream pools configured. Add an
backends-empty-suffix = block to gateway.toml and restart.

backends-fallback-offline-title = fallback_offline: served when every backend for a known model in this pool is down
backends-fallback-offline-badge = offline ↩ { $model }
backends-pool-empty = No backends in this pool.

backends-status-down = down
backends-status-saturated = saturated
backends-status-up = up

backends-inflight-label = inflight { $load }
backends-activity-summary = 15m { $m15 } · 30m { $m30 } · 60m { $m60 }
backends-no-models = no models advertised
backends-aliases-label = aliases:

backends-alias-target-title = alias → { $target }
backends-alias-disabled-label = { $name } (disabled)
backends-alias-disabled-title = bare alias disabled — this backend serves multiple models; give it an explicit target (map form)
backends-alias-bare-title = alias → this backend's model

# Backend CRUD editor (add/edit/delete backends stored in the DB topology).
backends-manage-heading = Manage backends
backends-manage-description = Add, edit, or remove upstream backends. Changes are saved to the database but only take effect once you click "Apply changes".
backends-apply-changes = Apply changes
backends-add-heading = Add backend
backends-field-name = Name
backends-field-base-url = Base URL
backends-field-api-key = API key
backends-field-api-key-placeholder = API key (stored encrypted)
backends-field-api-key-keep = leave blank to keep current key
backends-field-api-key-env = API key env var (fallback)
backends-field-health-path = Health path
backends-field-weight = Weight
backends-field-max-inflight = Max in-flight
backends-field-pool = Pool
backends-field-pool-none = (none)
backends-field-pool-hint = Assigns this backend to one pool. A backend in several pools collapses to the one chosen here.
backends-field-models = Models (comma-separated)
backends-field-aliases = Aliases (name=target per line)
backends-field-probe-models = Discover models from /models probe
backends-field-supports-edit = Supports image editing
backends-save-backend = Save backend
backends-add-backend = Add backend
backends-delete-backend = Delete
backends-error-name-required = backend name is required
backends-error-base-url-required = base URL is required
backends-saved = saved backend `{ $name }` — click "Apply changes" to reload
backends-deleted = deleted backend `{ $name }` — click "Apply changes" to reload

# Duplicate-name guard on the Add-backend form: the name is the primary key, so
# saving an existing one would silently replace that backend (and move it out of
# its pool). The first click warns, the second confirms.
backends-error-name-exists = a backend named `{ $name }` already exists — click "Add backend" again to overwrite it, or change the name
backends-overwrite-hint = This name already exists. Saving again OVERWRITES the existing backend — its base URL, API key, models, aliases and pool assignment. Change the name to add a second backend instead.

# An alias that is configured but would not route. The tooltip names the models
# the backend actually advertises, because the usual cause is a target id typed
# from memory ("qwen-32b") against a server that reports its full repo path.
backends-alias-unresolved-title = BROKEN: this alias points at `{ $target }`, which this backend does not serve, so requests for it fail.
backends-alias-nothing-title = BROKEN: this backend advertises no models, so a bare alias has nothing to bind to.
backends-alias-serves = It serves: { $models }
backends-alias-serves-nothing = It currently serves no models.
# Save-time check on the aliases textarea.
backends-alias-target-unknown = saved, but these alias targets are not served by this backend: { $targets } — it serves { $models }. Those aliases will not route until the target matches exactly.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
backends-auth-failed = key rejected
backends-auth-failed-title = The upstream rejected the health probe's credentials (401/403), so model discovery is off — nothing new can become routable through this backend. Check the API key; if it uses an env var, check that the variable is actually set.
backends-no-models-title = This backend advertises no models, so nothing routes to it and a bare alias has nothing to bind to. Usually a health probe that never returned data.
# Maintenance switch.
backends-enabled-label = Serving traffic
backends-enabled-hint = Turn off to drain this backend for maintenance. Takes effect immediately — no "Apply changes" needed. Its models stay known, so the pool's other backends take over and clients see a temporary outage, never "model not found".
backends-enabled-on = backend `{ $name }` is serving traffic again
backends-enabled-off = backend `{ $name }` drained for maintenance — no new requests will be routed to it
backends-status-drained = maintenance
backends-status-drained-title = Drained for maintenance: this backend is reachable but the router skips it. Turn "Serving traffic" back on to return it to rotation.
# "Test connection": call the upstream with what is typed in the editor.
backends-test-button = Test connection
backends-test-hint = Calls this URL with the credentials above. Nothing is saved.
backends-test-insert-hint = Reported model ids — click one to complete the alias line your cursor is on:
backends-test-ok = Reachable, authenticated ({ $source }), { $count } models discovered.
backends-test-ok-no-models = Reachable and authenticated ({ $source }), but the response is not an OpenAI /models envelope, so discovery cannot read it. This backend can only serve the ids you list under "Models".
backends-test-auth-failed = Rejected with HTTP { $status }: the credential was refused ({ $source }). Model discovery stays off until this is fixed, which leaves the backend advertising nothing.
backends-test-http-error = { $url } answered HTTP { $status }.
backends-test-unreachable = Could not reach { $url }: { $err }
backends-test-timeout = { $url } did not answer within { $secs }s.
backends-test-key-typed = using the key typed above
backends-test-key-stored = using the stored key
backends-test-key-env = from env { $var }
backends-test-key-env-unset = env { $var } is NOT SET — the request went out with no credential
backends-test-key-none = no credential sent
# Where the API key comes from (U5).
backends-key-env-badge = key: env { $var }
backends-key-env-title = This backend has no stored key; it reads one from this environment variable, which is currently set.
backends-key-env-unset-badge = env { $var } NOT SET
backends-key-env-unset-title = This backend has no stored key and the environment variable it names is not set in the gateway process, so it sends no credential at all. If the upstream requires one, every probe gets 401, model discovery stays off, and the backend advertises nothing — enter the key in the "API key" field instead, or set the variable and restart.
# Live name-clash note on the add form (U9).
backends-name-taken = A backend with this name already exists — saving would overwrite it, including its base URL, key, models and pool. Pick a different name to add a second backend.
