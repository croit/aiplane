# The upstreams surface: what a pool is, and the board's column heads.
upstreams-heading = Upstreams
upstreams-description = Pools group backends by kind and picker strategy. Health, load and served models are probed live. Topology edits are saved to the database and applied to the running registry immediately.
upstreams-add-pool = Pool
upstreams-add-backend = Backend
upstreams-unassigned-heading = Unassigned
upstreams-unassigned-description = Backends not assigned to any pool. Add one to a pool to route traffic to it.
upstreams-empty = No pools or backends configured yet. Add a pool or a backend to get started.
upstreams-cancel = Cancel
upstreams-edit-backend = Edit backend
upstreams-comp-limits = limits
upstreams-edit-pool = Edit pool

# The concourse board: column heads, the state lamp vocabulary, and the row
# actions.
board-col-name = Backend
board-col-state = State
board-col-load = Load
board-col-requests = Requests/h
board-summary-on-time = on time
board-summary-degraded = degraded
board-summary-down = down
board-summary-maintenance = maintenance
board-on-time = On time
board-on-time-detail = Healthy, serving traffic
board-saturated = Saturated
board-saturated-detail = { $inflight } of { $max } inflight requests in use
board-down = Down
board-down-detail = The health probe is failing
board-maintenance = Maintenance
board-maintenance-detail = Drained — takes no new traffic
board-auth = Key rejected
board-auth-detail = The upstream refused the API key
board-no-probe = No probe
board-no-probe-detail = Not probed yet
board-models-unit = models
board-legend-on-time = on time — healthy, serving
board-legend-degraded = degraded — saturated or key issue
board-legend-down = down — unreachable
board-legend-maintenance = maintenance — drained
board-enabled = Serving
board-drained = Drained
board-edit = Edit

board-detail-endpoint = Endpoint
board-detail-models = Models
board-detail-aliases = Aliases
board-detail-activity = Traffic
board-key-none = no key configured
board-models-none = none discovered
board-models-more = +{ $count } more
board-pool-account = Account
board-pool-managed = This pool is managed on the OpenRouter tab.

board-filter-placeholder = Filter backends, models, aliases…
board-filter-clear = Clear
board-filter-empty = No backend and no pool matches this filter.
board-edit-named = Edit backend { $name }
upstreams-saved = Saved — live now.
board-model-open-catalog = Open this model in the catalog
