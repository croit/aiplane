# Personal Dot management permission

## Implemented behavior

Personal Dot lifecycle management uses an independent group permission,
`can_manage_dots`. It does not grant system-principal or ordinary agent
management. The existing active limit remains four by default. Administrators
choose the actual gateway groups that receive Dot management; a default group
can include every authenticated user. Fresh groups default to false, and admin
or bootstrap-admin membership implies the permission. Agent-management rights
do not imply it.

The existing lifecycle gate is replaced on list, create and active-state routes.
Current-user JSON exposes the resolved permission, and Dots navigation uses
it. Existing owner-scoped access, memory, activity, private-chat and background
binding checks remain their existing boundaries. Revoking management permission
blocks lifecycle management immediately after RBAC reload; this flag does not
revoke tool grants, deactivate existing Dots or independently stop existing
runs. Execution remains bounded by current owner grants, settings, Dot active
state, selected tools and approval rules.

## Shared mechanisms

Reuse `gateway_groups`, its flag setters and `GroupSnapshot`, the existing
`Resolver`, default-group/OIDC membership, bootstrap-admin resolution, admin
GET/PUT group handlers and live snapshot reload. Extend the existing `Me`
wire shape and `AdminGroupForm`; do not add a permission engine, selection
screen or role store. The personal-Dot migration includes the group column, with pinned checksum
and schema coverage. No new Cargo dependency is needed. The Dot settings section contains the
active-count limit and has no feature master switch; section enablement
metadata is absent, and navigation uses the resolved management permission.

## Actual-data walkthrough

An administrator opens `/admin/groups` and edits an existing group such as the
seeded Engineering group. Beside the existing agent-management checkbox, the
form offers personal-Dot management. Enable that flag while leaving agent
management disabled, then save using the existing group save flow. A member's
current-user response shows Dot management true and agent management false.
The Dots entry becomes available; the member creates and deactivates an owned
Dot through the ordinary page, subject to the active limit. They cannot create
or manage unrelated principals. Turning the flag off and saving immediately
removes the management permission. Admin groups display both implied rights.

All new labels and the Dot-specific denied-management message use six Fluent
catalogues. Existing group labels, save errors, model/tool grants and limits
remain the shared controls.

## Verification coverage

Tests cover independent flags, explicit and default-group grants, bootstrap
and admin implication, false defaults, immediate reload/revocation, Dot-only
lifecycle without agent management, existing owner isolation and active caps.
Migration coverage checks existing group grants survive and do not acquire Dot
management implicitly. Chrome uses synthetic group members to exercise the
actual checkbox and Dot page. Documentation distinguishes management permission
from execution grants and keeps operator setup accurate.
