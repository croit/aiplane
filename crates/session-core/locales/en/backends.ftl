# The upstream board and the backend editor: the pool band's status word, the
# editor's fields, and the connection-test outcomes.
backends-status-up = up

# Backend CRUD editor (add/edit/delete backends stored in the DB topology).
backends-apply-changes = Apply changes
backends-field-name = Name
backends-field-base-url = Base URL
backends-field-api-key = API key
backends-field-api-key-keep = leave blank to keep current key
backends-field-api-key-stored = Stored key: { $hint }
backends-field-pool = Pool
backends-field-pool-none = (none)
backends-field-pools = Pools
backends-field-pools-hint = One pool per kind the account serves. A kind joins an existing empty pool of that kind, gets a new pool created in the same save, or is not served. Pools that already hold another backend are not offered — mixing them pins that backend to this allowlist.
backends-pool-not-served = Not served
backends-pool-new = New: openrouter-{ $kind }
backends-save-backend = Save backend
backends-delete-backend = Delete
backends-delete-confirm = Delete backend “{ $name }”? Its pool keeps the other backends.
backends-advanced = Advanced settings

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
backends-no-models-title = This backend advertises no models, so nothing routes to it and a bare alias has nothing to bind to. Usually a health probe that never returned data.
backends-no-models = no models
backends-key-env-badge = key: env { $var }
backends-key-env-unset-badge = env { $var } NOT SET
backends-enabled-hint = Turn off to drain this backend for maintenance. Takes effect immediately — no "Apply changes" needed. Its models stay known, so the pool's other backends take over and clients see a temporary outage, never "model not found".
backends-enabled-label = Serving traffic
backends-activity-summary = 15m { $m15 } · 30m { $m30 } · 60m { $m60 }
backends-alias-target-title = alias → { $target }
backends-alias-missing-title = alias → { $target } — this backend does not advertise that model
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
openrouter-search = Search models
openrouter-select-all = Select all
openrouter-deselect-all = Deselect all
openrouter-selected-count = { $count } selected
openrouter-model = Model
openrouter-price = List price
openrouter-context = Context
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
openrouter-add-heading = Add OpenRouter
openrouter-add-description = One key, everything set up: the backend plus a pool for every model kind it serves (chat, transcription, speech, System One, embedding, image, rerank), named openrouter-<kind>. Existing pools with that name are joined. Model selection and price ceilings come next in the OpenRouter panel.
openrouter-add-save = Add OpenRouter
openrouter-kinds = Model kinds
openrouter-kind-selected-count = { $count } selected
openrouter-kind-catalog-count = { $count } in catalog
openrouter-kind-not-enabled = Not enabled
openrouter-key-change = Change key
openrouter-key-placeholder = New API key
openrouter-key-save = Save key
openrouter-key-saved = Key updated
openrouter-sort = Sort
openrouter-save-changed = Save selection · { $count } changed
openrouter-dirty-confirm = { $kind } has unsaved changes. Discard them?
openrouter-provider-field = Provider
openrouter-provider-openai = OpenAI-compatible
openrouter-provider-openrouter = OpenRouter
openrouter-provider-hint = OpenRouter reads the account catalog and enforces the per-model price ceiling on every request. Other providers are plain OpenAI-compatible HTTP.
openrouter-ceiling-required = Every selected model needs both price ceilings.
openrouter-max-price = Max $/1M
openrouter-voices = Voices
