# Admin rate-limit / quota editor (/admin/limits).
limits-heading = Rate limits & quotas
limits-add-heading = Add or update a limit
limits-field-subject = Applies to
limits-field-model = Model
limits-field-dimension = Limit
limits-field-window = Per
limits-field-value = Value
limits-add-submit = Save limit
limits-subject-global = Everyone (default)
limits-subject-role = Role
limits-subject-user = User
limits-dim-requests = Requests
limits-dim-tokens = Tokens
limits-dim-cost = Cost ({ $cur })
limits-dim-cost-short = Cost
limits-win-hour = Hour
limits-win-day = Day
limits-win-week = Week
limits-win-month = Month
limits-col-subject = Applies to
limits-col-scope = Model
limits-col-limit = Limit
limits-col-window = Window
limits-none = No limits configured — everyone is unlimited.
limits-all-models = all models
limits-delete = Delete
limits-edit = Edit
limits-edit-heading = Edit limit
limits-edit-submit = Save changes
limits-saved = saved limit for { $subject }
limits-subject-token = API token
limits-subject-system = Agent

# SPA rule table: its heading, the "managed by" column, and the delete prompt.
limits-delete-confirm = Delete this rule?
limits-intro = Cap how many requests, tokens, or how much spend a caller may use over a rolling window. Rules resolve most-specific-first: a user's own rule wins, else the most generous of their roles, else the global default. With no rules, everyone is unlimited. A rule on an API token is an additional ceiling checked alongside its owner's budget, so it can only narrow what that token may spend. Only pools with enforce_limits = true count toward a limit (self-hosted pools set enforce_limits = false are still recorded on /usage but exempt), and a user's whole budget is shared across their API tokens, chat, and scheduled runs. A rule scoped to one model (or a `prefix-*` family) overrides the aggregate in its window: for a call on that model only the most specific matching rule counts — unless the aggregate is marked as an overall ceiling, in which case it caps additionally.
limits-field-subject-id = Role / user / token / agent
limits-field-subject-id-ph = Choose …
limits-col-value = Value
limits-col-actions = Actions
limits-deleted = removed limit
limits-field-total = Overall ceiling
limits-field-total-hint = When set, this rule caps additionally — even where a more specific rule allows more. When unset, the most specific matching rule wins for the called model.
limits-field-model-hint = Empty means all models; a trailing * scopes a family (e.g. claude-*).
limits-col-kind = Kind
limits-kind-total = Total
limits-kind-override = Override
