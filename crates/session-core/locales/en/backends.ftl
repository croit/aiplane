# Strings owned by `gateway/src/rama_server/pages/backends.rs` — the
# read-only `/admin/backends` operator view of the upstream pools.


backends-status-down = down
backends-status-up = up

backends-inflight-label = inflight { $load }

# Backend CRUD editor (add/edit/delete backends stored in the DB topology).
backends-apply-changes = Apply changes
backends-field-name = Name
backends-field-base-url = Base URL
backends-field-api-key = API key
backends-field-api-key-keep = leave blank to keep current key
backends-field-api-key-stored = Stored key: { $hint }
backends-field-pool = Pool
backends-field-pool-none = (none)
backends-save-backend = Save backend
backends-delete-backend = Delete

# An alias that is configured but would not route. The tooltip names the models
# the backend actually advertises, because the usual cause is a target id typed
# from memory ("qwen-32b") against a server that reports its full repo path.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.
backends-status-drained = maintenance

# SPA backend rows: the drain switch, the per-hour load figure, and the
# delete confirmation.
backends-add-heading = Add backend
backends-name-taken = A backend with this name already exists — saving would overwrite it, including its base URL, key, models and pool. Pick a different name to add a second backend.
backends-field-api-key-placeholder = API key (stored encrypted)
backends-field-api-key-env = API key env var (fallback)
backends-field-health-path = Health path
backends-field-pool-hint = Assigns this backend to one pool. A backend in several pools collapses to the one chosen here.
backends-field-weight = Weight
backends-field-max-inflight = Max in-flight
backends-field-models = Models (comma-separated; blank = discover from /models)
backends-field-aliases = Aliases (name=target per line)
backends-field-extra-headers = Extra headers (name: value per line)
backends-field-extra-headers-hint = Sent with every request to this backend, after the API key. For example the anthropic-version: 2023-06-01 that Anthropic requires.
backends-field-supports-edit = Supports image editing
backends-status-saturated = saturated
backends-auth-failed-title = The upstream rejected the health probe's credentials (401/403), so model discovery is off — nothing new can become routable through this backend. Check the API key; if it uses an env var, check that the variable is actually set.
backends-auth-failed = key rejected
backends-no-models-title = This backend advertises no models, so nothing routes to it and a bare alias has nothing to bind to. Usually a health probe that never returned data.
backends-no-models = no models advertised
backends-key-env-badge = key: env { $var }
backends-key-env-unset-badge = env { $var } NOT SET
backends-enabled-hint = Turn off to drain this backend for maintenance. Takes effect immediately — no "Apply changes" needed. Its models stay known, so the pool's other backends take over and clients see a temporary outage, never "model not found".
backends-enabled-label = Serving traffic
backends-activity-summary = 15m { $m15 } · 30m { $m30 } · 60m { $m60 }
backends-aliases-label = aliases:
backends-alias-target-title = alias → { $target }
backends-alias-disabled-title = bare alias disabled — this backend serves multiple models; give it an explicit target (map form)
backends-alias-disabled-label = { $name } (disabled)
backends-fallback-offline-title = fallback_offline: served when every backend for a known model in this pool is down
backends-fallback-offline-badge = offline ↩ { $model }
backends-pool-empty = No backends in this pool.

backends-test-button = Test connection
backends-test-hint = Calls this URL with the credentials above. Nothing is saved.
backends-test-insert-hint = Reported model ids — click one to complete the alias line your cursor is on:
backends-test-ok = Reachable, authenticated ({ $source }), { $count } models discovered.
backends-test-ok-no-models = Reachable and authenticated ({ $source }), but the response is neither an OpenAI nor a TypeSafe /models list, so discovery cannot read it. This backend can only serve the ids you list under "Models".
backends-test-auth-failed = Rejected with HTTP { $status }: the credential was refused ({ $source }). Model discovery stays off until this is fixed, which leaves the backend advertising nothing.
backends-test-http-error = { $url } answered HTTP { $status }.
backends-test-upstream-said = Upstream response: { $detail }
backends-test-unreachable = Could not reach { $url }: { $err }
backends-test-timeout = { $url } did not answer within { $secs }s.
backends-test-key-typed = using the key typed above
backends-test-key-stored = using the stored key
backends-test-key-env = from env { $var }
backends-test-key-env-unset = env { $var } is NOT SET — the request went out with no credential
backends-test-key-none = no credential sent
backends-error-base-url-required = base URL is required

# What detection made of a backend. Never shown as a setting — only where it
# changes what the operator should do.
backends-parallel-mismatch = server runs { $parallel } at a time
backends-detect-profile = identified: { $profile }

# OpenRouter catalog: one backend, one key, the account's models, and a
# purchase-price ceiling per selected model.
openrouter-heading = OpenRouter
openrouter-description = One OpenRouter connection. Pick the models this gateway may use and the maximum you will pay for each; a model with no provider under its ceiling is not offered.
openrouter-connected = Connected
openrouter-key-label = Key
openrouter-credits = Credits
openrouter-refresh = Refresh prices
openrouter-backend = Backend
openrouter-pool = Pool
openrouter-search = Search models
openrouter-select-all = Select all
openrouter-deselect-all = Deselect all
openrouter-selected-count = { $count } selected
openrouter-model = Model
openrouter-price = List price
openrouter-context = Context
openrouter-tools = Tools
openrouter-ceiling-prompt = Max input $/1M
openrouter-ceiling-completion = Max output $/1M
openrouter-strategy = Provider strategy
openrouter-strategy-cheapest = Cheapest
openrouter-strategy-fastest = Fastest
openrouter-strategy-balanced = Balanced
openrouter-min-throughput = Min tokens/s
openrouter-available = Available
openrouter-over-ceiling = Over ceiling
openrouter-save = Save selection
openrouter-saved = Selection saved.
openrouter-no-models = No models match this filter.
openrouter-error = OpenRouter: { $message }
openrouter-select-model-first = Select at least one model.
openrouter-endpoints = Providers
openrouter-provider = Provider
openrouter-throughput = Tokens/s
openrouter-uptime = Uptime
openrouter-add-backend = Add OpenRouter backend
openrouter-provider-field = Provider
openrouter-provider-openai = OpenAI-compatible
openrouter-provider-openrouter = OpenRouter
openrouter-provider-hint = OpenRouter reads the account catalog and enforces the per-model price ceiling on every request. Other providers are plain OpenAI-compatible HTTP.
openrouter-ceiling-required = Every selected model needs both price ceilings.
openrouter-pool-empty = This backend is not in a pool yet. Add it to a chat, embedding, speech, transcription, image, rerank or System One pool first.
openrouter-mixed-pool = This pool also contains { $backends }. A pool has one model list, so saving here pins theirs too; put OpenRouter in its own pool.
openrouter-max-price = Max $/1M
openrouter-voices = Voices
