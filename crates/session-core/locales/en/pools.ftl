# Strings owned by `gateway/src/rama_server/pages/pools.rs` — the
# `/admin/pools` CRUD editor for the DB-backed upstream pool topology.

pools-heading = Upstream pools

pools-field-name = Name
pools-field-kind = Kind
pools-field-models = Served models (allowlist, comma-separated)
pools-field-backends = Backends
pools-save-pool = Save pool
pools-add-pool = Add pool
pools-delete-pool = Delete

# SPA pool cards: the two one-line summaries and the delete confirmation.
pools-summary-backends = { $count } backend(s): { $list }
pools-summary-models = { $count } model(s): { $list }
pools-delete-confirm = Delete pool { $name }? Apply the topology afterwards.
