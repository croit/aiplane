# Strings owned by `gateway/src/rama_server/pages/admin.rs` — the
# `/admin/models` page: default-model pickers plus one filterable list of every
# advertised model, each with a single consolidated editor (pricing, context
# window, reasoning style + budgets/efforts, capabilities, sampling defaults).

admin-heading = Models
admin-models-routing-heading = Models & routing
admin-models-routing-intro = Configure model infrastructure, catalog behavior, defaults, and automatic routing in one place.
admin-models-tab-upstreams = Upstreams
admin-models-tab-catalog = Model catalog
admin-models-tab-defaults = Default models
admin-models-tab-routing = Automatic routing

# List column headers.
admin-col-model = Model

# Collapsed-row values.
admin-not-configured = not configured

# "Configured" facet badges.
admin-badge-ctx = CTX

# Editor.
admin-save-model = Save model
admin-edit-model = Edit
admin-edit-model-page-title = Edit model
admin-model-not-found = No such model. It may no longer be advertised by any backend.
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
admin-search-provider-tavily = Tavily Search API
admin-search-searxng-url-label = SearXNG base URL
admin-search-searxng-url-placeholder = https://searxng.example.com
admin-search-brave-key-label = Brave API key
admin-search-brave-key-placeholder = leave blank to keep the current key
admin-search-tavily-key-label = Tavily API key
admin-search-tavily-key-placeholder = leave blank to keep the current key
admin-search-save = Save web search
admin-search-saved = web-search settings saved

# ─── SvelteKit admin SPA ─────────────────────────────────────────────────────
# The admin shell's role guard, the /admin/comfyui workflow catalog, and the
# bits of the /admin/models editor the SPA renders that the legacy page didn't.

admin-needs-admin-role = These pages need the admin role.
admin-overwrite-existing = overwrite existing
admin-comfyui-reload = Reload catalog
admin-comfyui-reloaded = Catalog reloaded — { $count } workflow(s) loaded.
admin-comfyui-empty = No workflows loaded — check the content directory.
admin-comfyui-heading = ComfyUI workflow catalog
admin-comfyui-page-title = ComfyUI — Workflow catalog
admin-comfyui-intro = Headless ComfyUI worker. The gateway exposes each loaded workflow as a comfyui_<id> tool the model can call. Users never see ComfyUI itself.
admin-comfyui-not-configured = Not configured
admin-comfyui-not-configured-help = Enable ComfyUI under Settings — set its base URL and workflow directory — then restart the gateway to load the workflow catalog.
admin-comfyui-operator-config = Operator configuration
admin-comfyui-worker-url = Worker base URL
admin-comfyui-content-directory = Content directory
admin-comfyui-timeout = Workflow timeout
admin-comfyui-poll-interval = Queue poll interval
admin-comfyui-config-help = The content directory is operator-managed and not part of the public repository. Edit manifests there and click Reload above to apply changes — no restart needed.
admin-comfyui-loaded-workflows = Loaded workflows
admin-comfyui-node = node { $id }
admin-comfyui-parameters = Parameters
admin-comfyui-required = required
admin-comfyui-recent-jobs = Recent jobs
admin-comfyui-reloaded-skipped = Catalog reloaded — { $count } workflow(s) loaded, { $skipped } skipped.
admin-comfyui-max-concurrent = Concurrent jobs
admin-comfyui-tab-workflows = Workflows
admin-comfyui-tab-jobs = Job runs
admin-comfyui-jobs-page-title = ComfyUI — Job runs
admin-comfyui-jobs-intro = Every workflow the models have run recently, newest first — what it produced, how long it took, and the conversation that asked for it.
admin-comfyui-jobs-window = The { $count } most recent runs the gateway recorded.
admin-comfyui-jobs-empty = No runs recorded yet.
admin-comfyui-worker-status = Worker
admin-comfyui-worker-reachable = Reachable
admin-comfyui-worker-unreachable = Unreachable
admin-comfyui-worker-checking = Checking…
admin-comfyui-worker-queue = { $running } running · { $pending } queued
admin-comfyui-worker-software = ComfyUI { $version } · Python { $python } · PyTorch { $torch }
admin-comfyui-worker-vram = { $free } free of { $total }
admin-comfyui-search-placeholder = Search workflows and parameters
admin-comfyui-search-empty = No workflow matches that search.
admin-comfyui-filename-prefix = Output prefix
admin-comfyui-required-count = { $required } of { $total } required
admin-comfyui-no-params = This workflow takes no parameters.
admin-comfyui-detail-empty = Pick a workflow to see the contract the model sees.
admin-comfyui-param-column = Parameter
admin-comfyui-param-description-column = Description
admin-comfyui-filter-all = All
admin-comfyui-filter-completed = Completed
admin-comfyui-filter-pending = Pending
admin-comfyui-filter-failed = Failed
admin-comfyui-stats-heading = Reliability by workflow
admin-comfyui-col-workflow = Workflow
admin-comfyui-col-runs = Runs
admin-comfyui-col-failed = Failed
admin-comfyui-col-median = Median
admin-comfyui-col-status = Status
admin-comfyui-col-duration = Duration
admin-comfyui-col-when = Started
admin-comfyui-col-result = Result
admin-comfyui-job-open = Open conversation
admin-comfyui-job-still-running = still running
admin-comfyui-refresh = Refresh
admin-clear-overrides-confirm = Drop all stored overrides for { $model }?
admin-cap-no-fallback = (none)

admin-page-title = Models — AIplane

admin-no-models = No models advertised yet. Once an upstream backend is reachable, it'll appear here.

admin-filter-placeholder = Filter models…

admin-filter-all = All

admin-filter-chat = chat

admin-filter-other = other kinds

admin-filter-aliases = aliases

admin-filter-configured = configured only

admin-col-kind = Kind

admin-col-price = Price in/out

admin-col-context = Context

admin-col-reasoning = Reasoning

admin-col-configured = Configured

admin-value-default = default

admin-value-na = n/a

admin-alias-inherits = inherits target settings

admin-reasoning-auto-resolved = Auto → { $style }

admin-badge-price = PRICE

admin-badge-budget = BUDGET

admin-badge-caps = CAPS

admin-badge-toml = TOML

admin-other-price-note = Sampling, reasoning and context don't apply to this kind — only pricing, for cost accounting.

admin-toml-placeholder-header = # Common keys (vLLM/OpenAI):

admin-reasoning-style-label = Reasoning style

admin-reasoning-style-aria = Reasoning style

admin-reasoning-auto = Auto

admin-reasoning-none = none

admin-reasoning-qwen = Qwen (vLLM)

admin-reasoning-openai = OpenAI

admin-reasoning-glm = GLM / z.AI

admin-reasoning-anthropic = Anthropic
admin-reasoning-ollama = Ollama

admin-effort-standard = Standard

admin-effort-deep = Deep

admin-effort-max = Max

admin-budget-placeholder = default

admin-budget-hint = Max thinking tokens per effort level. Blank = backend default (uncapped). Fast disables thinking.

admin-effort-default-option = (default)

admin-effort-hint = Reasoning effort per level. Blank = built-in default. Fast disables thinking.

admin-saved-model = saved `{ $model }` — effective immediately

admin-cleared-defaults = cleared overrides for `{ $model }`

admin-price-label = { $cur }/{ $unit }

admin-price-unit-tokens = 1M tokens

admin-price-unit-images = image

admin-price-unit-characters = character

admin-price-unit-seconds = second

admin-alias-chip = alias

admin-defaults-intro = The model used when a request names none — the pre-selection in the chat/voice pickers and the API default (unlike the Unknown-model fallbacks on the Upstreams page, which apply when a request names a model no pool serves). Blank = the first available model.

admin-defaults-chat-label = Chat

admin-defaults-voice-label = Voice (transcription)

admin-defaults-image-label = Image generation

admin-defaults-embedding-label = Embedding (RAG)

admin-defaults-first-option = First available

admin-defaults-saved = default model set to `{ $model }`

admin-defaults-cleared = default model cleared

admin-capabilities-heading = Capabilities

admin-cap-vision = Vision

admin-cap-structured-output = Structured output

admin-cap-audio-input = Audio input

admin-cap-pdf-input = PDF input

admin-cap-parallel-tools = Parallel tools

admin-cap-unknown = Unknown

admin-cap-enabled = Enabled

admin-cap-disabled = Disabled

admin-cap-fallback-vision = Fallback for vision

admin-cap-fallback-tools = Fallback for tools

admin-search-intro = Choose the backend for `search_web`. SearXNG needs a base URL; Brave and Tavily need API keys, which are encrypted at rest. If a provider's quota is exhausted, another configured provider is tried.

admin-search-brave-key-set = A key is stored (encrypted).

admin-search-brave-key-unset = No key stored.

admin-search-brave-key-clear = Remove the stored key
admin-search-tavily-key-set = A Tavily key is stored (encrypted).
admin-search-tavily-key-unset = No Tavily key stored.
admin-search-tavily-key-clear = Remove the stored Tavily key
admin-search-tavily-enabled = Enable Tavily
admin-search-status-active = Active
admin-search-status-inactive = Inactive

# Context window provenance. The value is discovered from the serving backend
# where the backend reports one; an operator may still override it, and may
# legitimately lower it. Raising it above what the server serves is the one
# case worth warning about: the prompt is not compacted here, it is truncated
# there, in silence.
admin-context-detected = Detected: { $window } tokens
admin-context-unreported = This backend does not report a context window for this model. Set it here, or check the server.
admin-context-exceeds-detected = This backend reports { $window } tokens. Higher values are not compacted — the server truncates them silently.
auto-route-heading = Automatic model routes
auto-route-description = Expose a standard model alias that selects the best eligible model for each request.
auto-route-add = Add automatic route
auto-route-privacy = The selector receives the request content needed to classify it, even when the selected generation model is local.
auto-route-empty = No automatic routes are configured.
auto-route-needs-selector = Add a System One upstream before creating an automatic route.
auto-route-needs-candidates = Add at least two distinct chat models before creating an automatic route.
auto-route-open-upstreams = Open upstreams
auto-route-candidates = Candidates
auto-route-edit = Edit
auto-route-delete = Delete
auto-route-delete-confirm = Delete automatic route “{ $alias }”?
auto-route-deleted = Deleted automatic route “{ $alias }”.
auto-route-saved = Saved automatic route “{ $alias }”.
auto-route-alias = Model alias
auto-route-alias-help = Clients use this value in the standard model field.
auto-route-selector = Selector model
auto-route-selector-help = A model from a System One upstream that chooses a candidate key.
auto-route-editor-help = Start in shadow mode, review routing decisions, then activate the route when its policy is ready.
auto-route-objective = Optimization objective
auto-route-objective-quality = Quality
auto-route-objective-balanced = Balanced
auto-route-objective-cost = Cost
auto-route-rollout = Rollout mode
auto-route-rollout-shadow = Shadow (measure only)
auto-route-rollout-active = Active
auto-route-rollout-help = Shadow records decisions but sends traffic to the fallback. Active applies the selector's decision.
auto-route-confidence = Minimum confidence
auto-route-confidence-help = Lower-confidence decisions use the fallback target.
auto-route-timeout = Selector timeout (ms)
auto-route-instructions = Routing instructions
auto-route-instructions-help = Optional plain-language rules for the selector. No special syntax is required.
auto-route-instructions-placeholder = Example: Prefer expert for complex analysis and code; use fast for short, routine requests.
auto-route-candidate-add = Add candidate
auto-route-candidates-help = Describe each model by the requests it should handle. The selector sees the key and description, not the target model name.
auto-route-candidate-key = Opaque key
auto-route-candidate-target = Target model or static alias
auto-route-candidate-description = When this candidate should be used
auto-route-candidate-description-help = Required plain text. Describe the requests this candidate handles best; the selector sees this text, not the target model name.
auto-route-candidate-description-placeholder = Example: Short, routine requests where low latency and cost matter most.
auto-route-fallback = Fallback target
auto-route-session-affinity = Keep a session on its first selected model
auto-route-session-affinity-help = Reuses the effective target for the same authenticated client and session until the TTL expires.
auto-route-session-ttl = Session affinity TTL (seconds)
auto-route-cancel = Cancel
auto-route-save = Save route
auto-route-saving = Saving…
auto-route-decisions = Recent routing decisions
auto-route-result = Effective target
auto-route-latency = Selector latency
auto-route-reason-selected = Selected
auto-route-reason-shadow = Shadow suggestion
auto-route-reason-low-confidence = Low confidence
auto-route-reason-selector-error = Selector error
auto-route-reason-session-affinity = Session affinity
auto-route-error-unavailable = No configured target can currently serve this request. Check candidate availability and capabilities, then retry.
auto-route-error-internal = The automatic route could not be loaded. Check the gateway logs and try again.
auto-route-error-invalid-body = The automatic route request is malformed. Check the submitted fields and try again.
auto-route-error-invalid-config = The automatic route configuration is invalid. Check its candidates, fallback, confidence, and timeouts.
auto-route-error-nested = Automatic routes cannot target other automatic routes. Choose static model aliases or model IDs instead.
auto-route-error-missing-alias = The request URL is missing the automatic route alias.
auto-route-error-not-found = The automatic route does not exist. Refresh the route list and try again.
admin-error-unknown-groups = { $count ->
    [one] No such group here: { $groups }. Create it under Admin → Groups first, or pick from the list.
   *[other] No such groups here: { $groups }. Create them under Admin → Groups first, or pick from the list.
}
