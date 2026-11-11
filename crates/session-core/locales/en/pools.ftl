# Strings owned by `gateway/src/rama_server/pages/pools.rs` — the
# `/admin/pools` CRUD editor for the DB-backed upstream pool topology.


pools-field-name = Name
pools-field-kind = Kind
pools-field-models = Served models (comma-separated)
pools-field-backends = Backends
pools-save-pool = Save pool
pools-delete-pool = Delete
pools-delete-confirm = Delete pool “{ $name }”? Its backends stay and become unassigned.

# SPA pool cards: the two one-line summaries and the delete confirmation.
pools-add-heading = Add pool
pools-fallbacks-heading = Unknown-model fallbacks
pools-fallbacks-description = The substitute when a request names a model no pool serves (unlike the per-feature default on the Defaults tab, which applies when a request names nothing). Blank = the miss returns 404.
upstreams-problems-heading = This pool is not fully working
pools-name-taken = A pool with this name already exists — saving would replace it, including its backends and models. Pick a different name to create a new pool.
pools-field-strategy = Strategy
pools-field-strategy-hint = prefix_affinity keeps one conversation on the replica that already holds its KV cache (best for chat/agent traffic across several GPUs — the others alternate turns between replicas and pay a full prefill each time), and still spreads when a backend is genuinely busier. least_inflight balances by current load; round_robin rotates by weight.
pools-field-fallback-offline = Offline fallback model
pools-field-fallback-offline-placeholder = served when every backend is down
pools-field-models-hint = When set, exactly these ids are served and the backends are not asked what they have — needed where /models lists something else or omits a model. Blank = discover the models from each backend's /models.
pools-field-models-chips = Advertised by this pool's backends — click to pin or unpin
pools-field-allowed-groups = Allowed groups
pools-field-allowed-groups-hint = AIplane groups allowed to see + use this pool's models. None selected = everyone. Admins always have access. Manage groups in Admin → Groups.
pools-field-voices = Voices (lang=voice per line)
pools-field-offer-voices = Selectable voices (one per line, users pick)
pools-no-backends = No backends defined yet. Add one on the Backends page first.
pools-field-enforce-limits = Enforce rate limits & quotas

upstreams-problem-no-backends = No backends assigned — nothing in this pool can serve a request.
upstreams-problem-all-drained = Every backend is drained for maintenance, so nothing routes here.
upstreams-problem-all-down = No backend is available: requests wait for one to return, then get a temporary-outage error.
upstreams-problem-auth = Credential rejected by: { $backends }. Model discovery is off for those, so they advertise nothing.
upstreams-problem-no-models = No models advertised by: { $backends }. Nothing routes to them, and a bare alias there binds to nothing.
upstreams-problem-broken-aliases = Aliases that route nowhere: { $aliases }. Each points at a model its backend does not serve, or has nothing to bind to.
upstreams-problem-partial-coverage = Served by only part of the pool: { $models }. Requests for these names use fewer replicas than you have.
upstreams-problem-unserved-allowlist = Listed under Models but served by nobody: { $models }.
upstreams-problem-missing-backends = Assigned backends that no longer exist: { $backends }.
