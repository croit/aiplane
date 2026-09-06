# Strings owned by `gateway/src/rama_server/pages/pools.rs` — the
# `/admin/pools` CRUD editor for the DB-backed upstream pool topology.

pools-page-title = Upstream pools — LLM Gateway
pools-heading = Upstream pools
pools-description = Group backends into pools by kind and picker strategy. Changes are saved to the database but only take effect once you click "Apply changes".

pools-fallbacks-heading = Unknown-model fallbacks
pools-fallbacks-description = The substitute when a request names a model no pool serves (unlike the per-feature default on the Models page, which applies when a request names nothing). Blank = the miss returns 404.

pools-add-heading = Add pool
pools-field-name = Name
pools-field-kind = Kind
pools-field-strategy = Strategy
pools-field-fallback-offline = Offline fallback model
pools-field-fallback-offline-placeholder = served when every backend is down
pools-field-models = Served models (allowlist, comma-separated)
pools-field-models-hint = When set, only these ids are served from a probing backend — the rest are shown struck-through. Blank = serve everything the backend reports.
pools-field-allowed-groups = Allowed groups
pools-field-allowed-groups-hint = Comma-separated gateway groups allowed to see + use this pool's models. Blank = everyone. Admins always have access. Manage groups in Admin → Groups.
pools-field-voices = Voices (lang=voice per line)
pools-field-offer-voices = Selectable voices (one per line, users pick)
pools-field-backends = Backends
pools-no-backends = No backends defined yet. Add one on the Backends page first.
pools-field-gdpr = GDPR compliant
pools-field-nda = NDA covered
pools-field-enforce-limits = Enforce rate limits & quotas
pools-save-pool = Save pool
pools-add-pool = Add pool
pools-delete-pool = Delete

pools-error-name-required = pool name is required
pools-error-invalid-kind = invalid pool kind `{ $kind }`
pools-saved = saved pool `{ $name }` — click "Apply changes" to reload
pools-deleted = deleted pool `{ $name }` — click "Apply changes" to reload
pools-fallback-saved = { $kind } fallback set to `{ $model }`
pools-fallback-cleared = { $kind } fallback cleared

# Duplicate-name guard on the Add-pool form: saving a name that already exists
# would silently overwrite that pool, so the first click warns and the second
# one confirms.
pools-error-name-exists = a pool named `{ $name }` already exists — click "Add pool" again to overwrite it, or change the name
pools-overwrite-hint = This name already exists. Saving again OVERWRITES the existing pool — its backends, models, voices and compliance flags. Change the name to create a separate pool instead.
# Why the strategy choice matters for self-hosted replicas.
pools-field-strategy-hint = prefix_affinity keeps one conversation on the replica that already holds its KV cache (best for chat/agent traffic across several GPUs — the others alternate turns between replicas and pay a full prefill each time), and still spreads when a backend is genuinely busier. least_inflight balances by current load; round_robin rotates by weight.
# What the pool advertises, and how many replicas serve each name (U6/U7).
upstreams-coverage-heading = What clients see
upstreams-coverage-hint = Exactly the names GET /v1/models returns for this pool, each with how many of its backends can serve it right now. Anything below full means part of your hardware is idle for that name.
upstreams-coverage-full-title = Every backend in this pool serves this name.
upstreams-coverage-partial-title = Only some backends serve this name — requests for it use part of the pool, and the rest sits idle. Usually an alias whose target does not match what a backend advertises.
upstreams-coverage-none-title = No backend can serve this name right now. Requests for it get a temporary-outage error.

# The per-pool problem summary (U8).
upstreams-problems-heading = This pool is not fully working
upstreams-problem-no-backends = No backends assigned — nothing in this pool can serve a request.
upstreams-problem-all-drained = Every backend is drained for maintenance, so nothing routes here.
upstreams-problem-all-down = No backend is available: requests wait for one to return, then get a temporary-outage error.
upstreams-problem-auth = Credential rejected by: { $backends }. Model discovery is off for those, so they advertise nothing.
upstreams-problem-no-models = No models advertised by: { $backends }. Nothing routes to them, and a bare alias there binds to nothing.
upstreams-problem-broken-aliases = Aliases that route nowhere: { $aliases }. Each points at a model its backend does not serve, or has nothing to bind to.
upstreams-problem-partial-coverage = Served by only part of the pool: { $models }. Requests for these names use fewer replicas than you have.
upstreams-problem-unserved-allowlist = Listed under Models but served by nobody: { $models }.
upstreams-problem-missing-backends = Assigned backends that no longer exist: { $backends }.
# Live name-clash note on the add form (U9).
pools-name-taken = A pool with this name already exists — saving would replace it, including its backends and models. Pick a different name to create a new pool.
# The apply diff (U10).
upstreams-apply-diff-summary = Show what applying will change
upstreams-diff-pool-added = new pool { $pool } starts serving
upstreams-diff-pool-removed = pool { $pool } stops serving
upstreams-diff-pool-kind = pool { $pool }: kind { $from } → { $to }
upstreams-diff-pool-strategy = pool { $pool }: strategy { $from } → { $to }
upstreams-diff-backend-joins = { $backend } joins pool { $pool } and starts taking traffic
upstreams-diff-backend-leaves = { $backend } leaves pool { $pool } and stops taking traffic
upstreams-diff-backend-url = { $backend }: base URL { $from } → { $to } (its discovered models are re-probed)
upstreams-diff-backend-limits = { $backend }: weight { $weight }, max in-flight { $inflight }
upstreams-diff-backend-health-path = { $backend }: health path → { $to }
