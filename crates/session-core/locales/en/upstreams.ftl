# Sticky apply bar (shown while there are unapplied topology edits).
upstreams-apply-count = unapplied changes
upstreams-apply-note = — the runtime registry still serves the previous topology.

# A backend that exists in the DB but is not yet in the runtime registry.
upstreams-backend-pending = pending apply

# Toast after the SPA applies the pending topology.
upstreams-applied = Topology applied.
upstreams-heading = Upstreams
upstreams-description = Pools group backends by kind and picker strategy. Health, load and served models are probed live. Topology edits are saved to the database and take effect on Apply changes.
upstreams-add-pool = Pool
upstreams-add-backend = Backend
upstreams-unassigned-heading = Unassigned
upstreams-unassigned-description = Backends not assigned to any pool. Add one to a pool to route traffic to it.
upstreams-empty = No pools or backends configured yet. Add a pool or a backend to get started.
upstreams-delete-confirm = Really delete?
upstreams-cancel = Cancel
upstreams-model-withheld-title = Discovered via /models but withheld by this pool's model list — not served or advertised.
upstreams-models-inactive-hide = hide inactive
upstreams-models-inactive-pill = +{ $count } inactive
upstreams-edit-backend = Edit backend
upstreams-comp-gdpr = GDPR
upstreams-comp-nda = NDA
upstreams-comp-limits = limits
upstreams-edit-pool = Edit pool
