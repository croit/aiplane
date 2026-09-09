# Strings owned by `gateway/src/rama_server/pages/backends.rs` — the
# read-only `/admin/backends` operator view of the upstream pools.

backends-heading = Upstream backends

backends-status-down = down
backends-status-up = up

backends-inflight-label = inflight { $load }

# Backend CRUD editor (add/edit/delete backends stored in the DB topology).
backends-apply-changes = Apply changes
backends-field-name = Name
backends-field-base-url = Base URL
backends-field-api-key = API key
backends-field-api-key-keep = leave blank to keep current key
backends-field-pool = Pool
backends-field-pool-none = (none)
backends-save-backend = Save backend
backends-add-backend = Add backend
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
backends-drain-button = Drain
backends-undrain-button = Undrain
backends-requests-per-hour = { $count } req/h
backends-delete-confirm = Delete backend { $name }? Apply the topology afterwards.
