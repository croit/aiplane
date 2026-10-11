# Organization-owned Dots

## Status and scope

Organization ownership, group-derived membership, independent active limits and configuration are implemented. Current organization Dots can be created, shared, configured and published through the existing agent mechanisms. Current member writers can authorize and explicitly select each Dot’s own OAuth connection. Organization conversations, automation, external channels and memory remain unavailable until their respective implementation packages establish the required owner and live-authority checks.

The remaining sections distinguish the implemented ownership and configuration foundation from the adopted execution and knowledge design. Creating a gateway group or naming an ordinary agent after a company does not create organization ownership.

The confirmed product requirements are explicit organization ownership, an independent named Dot identity, selected organization or Dot credentials, limited autonomy with approvals, durable isolated knowledge, and operation through the web interface. A personal Dot may select its owner's account or a separate Dot account. Organization execution must never adopt a member's credentials, tool grants or personal memory.

[Dot identities](dots-identities.md) defines credential ownership and OAuth prerequisites. [Dots](dots.md), [background execution](dots-background.md), and [communication](dots-communication.md) define the existing personal behavior. This document defines the organization extension. Credential ownership follows the same adopted connection contract; implementation packages fix exact interfaces before parallel work.

## Verified source baseline

| Source | Existing mechanism | Consequence |
|---|---|---|
| `aiplane-agents/src/db/system_principals.rs`; migration `0077_agent_builder.sql` | Explicit principal grants; `created_by` is a text attribution field without a user foreign key. | Principal lifetime and execution authority can be independent of the creator. Attribution does not establish organization ownership. |
| `aiplane-agents/src/db/agents.rs` | Agent definitions, versions, draft revisions, audit, user/group shares and last-writer protection. | Reuse configuration and shares. Add explicit owner data rather than interpreting the creator or a share as the owner. |
| `aiplane-runtime/src/agents/access.rs::effective_access` | Admins have write; read/write shares require `can_manage_agents`; respond shares require no such flag. | Organization membership must narrow this access decision. Membership alone grants no share or tool authority. |
| `aiplane-core/src/server/db/gateway_groups.rs`; `json_admin.rs` | Named groups, OIDC mappings, group permissions and RBAC reload after changes. | Select actual group rows as membership sources. A group is not automatically an organization. |
| `aiplane-runtime/src/agents/profile.rs::RunProfile::load_from` | Active principal, compiled/pinned AgentSpec, model grant validation and principal-based tools. | Organization execution can reuse the principal run path. |
| `aiplane-agents/src/db/run_sessions.rs` | Principal sessions with `user_id=NULL`; agent version, parent and language association. | Organization histories belong to the Dot principal, not to the initiating member's personal chat. |
| `aiplane-runtime/src/agents/inbox.rs`; `json_inbox.rs` | Share-based standing, minimal responder context and existing durable suspension/resume. | Reuse the Inbox. Add membership validation to every organization standing and settlement path. |
| `aiplane-runtime/src/agents/model_call.rs`; `server/side_call.rs::Payer::agent` | Agent usage and root-agent budget enforcement. | Retain agent attribution and metering. An organization-wide aggregate budget is a separate change, not an implied existing capability. |
| `aiplane-tools/src/memory.rs`; `server/tools/mod.rs::ToolContext::person` | Personal memory tools require a user actor. | Organization execution cannot reuse personal memory by passing an employee ID. |
| `aiplane-agents/src/db/dot_memories.rs` | Personal Dot notes checked through the actual personal owner. | Preserve current ownership isolation when adding typed organization scopes. |
| `aiplane-agents/src/db/agent_state.rs` | Typed slots scoped to a run session. | Session state does not supply durable organization-wide memory. |
| `aiplane-runtime/src/server/scheduled/mod.rs`; `server/webhooks.rs` | Trigger records currently have a user owner and optional personal Dot binding. | Organization background execution needs a principal-owner bridge in these mechanisms. Existing user trigger ownership is insufficient. |

Principal tool inventories use `SystemPrincipal.grants` through `AppState::allowed_tools_for_principal`. The loaded grant set is a snapshot. `RunProfile` reloads it when loading a run; this alone is not proof that every invocation in an already running turn observes a subsequent revocation. Organization implementation must include the live check described below.

## Shared mechanisms

Reuse these mechanisms before adding their organization callers:

- Existing system principals, grants, model/pool constraints, typed `AgentSpec`, draft revision/CAS paths, audit and agent versions.
- Existing user/group shares, `effective_access`, current gateway-group resolution and admin recovery; one combined organization access decision for API, Inbox and runtime admission.
- Existing principal run sessions, `AgentRun`, `RunProfile`, `SessionDriver`, headless workers and exact-turn cancellation.
- Existing `chat_turn_suspensions`, staff settlement, request IDs and worker claims; no organization approval queue or separate approval engine.
- Existing MCP catalogue, actual tool metadata, generalized user/principal connection storage, OAuth flow, crypto sealing, generation checks and owner-keyed cache/refresh locking. Organization admission and selection extend this same store.
- Existing pinned/guarded outbound clients and capped readers for OAuth/MCP bodies; existing JSON caps for organization endpoints.
- Existing cron/webhook storage, run correlation, completion reconciliation and usage records; extend owner resolution rather than add another scheduler.
- Existing memory record shape and `MemoryKind`, bounded readers and CRUD; owner-scoped storage must become explicit before any organization memory caller lands.
- Existing agent budget controls, operator limits, usage records, selectors, settings forms, share editor and six-language Fluent catalogues.

New shared contracts are a typed organization owner, actual group-derived membership, an access resolver intersecting membership with existing shares, and a live organization execution check. They belong in the agent/runtime layers. Neutral connection ownership belongs in core. `session-core` must receive no organization types or agent-table queries.

## Owner and membership data model

### Organization metadata

`organizations` stores `id`, actual configured `name`, `description`, `revision`, `created_by`, `created_at` and `updated_at`. `created_by` records the operator action without a user foreign key; deleting that user preserves the organization.

`organization_groups(organization_id, gateway_group)` has foreign keys and a unique ordered pair. Membership is the intersection of these selected group names and the person's freshly resolved gateway groups. There is no separate user membership roster, OIDC mapping engine or role vocabulary. Removing a bound group rotates the organization revision in the same transaction; recreating its name does not restore the removed binding.

Organization creation and membership editing require existing gateway administration rights. Deleting a linked gateway group removes that membership source and immediately prevents access for people with no remaining qualifying group. An organization with no member groups is recoverable through explicit operator administration; it is not silently accessible to every user.

### One typed Dot owner

Adopted shared storage contract: generalize the existing personal Dot ownership record to a `dots` record linked to the existing agent/principal. Keep the existing name and active state, and use mutually exclusive `owner_user_id` and `organization_id` foreign keys with a CHECK requiring exactly one owner. Expose `DotOwner::User(id) | Organization(id)` in `aiplane-agents`.

The database enforces this exclusive owner and prevents changes to either owner or principal ID. Personal accessors and APIs require an actual user owner. Organization configuration uses the same agent and principal records, with their typed organization owner.

The owner is fixed at creation. Moving a Dot between user and organization is outside this slice because it changes credential, knowledge, history and payer boundaries. Shares do not change the owner. Group names and `created_by` do not change the owner.

The existing principal is the Dot's execution identity. Organization Dots use `Actor::Agent`; creating an organization Dot must not construct `Actor::PersonalDot` with a member ID. Organization-owned resources survive member or creator deletion. Organization deletion uses explicit lifecycle validation and cleanup; dangling executable principals are not acceptable.

### Credentials

Use the identity proposal's neutral `ConnectionOwner::User | Principal` and existing OAuth store. An organization Dot may select its own principal credential or a credential owned by an explicitly managed organization principal. Link that credential principal to its real organization with an FK-backed association. Do not allocate an organization credential principal until an actual shared-credential caller is included.

Bindings must prove that the selected principal is either the Dot principal or an authorized credential principal of the same organization. Missing, disabled or replaced connections fail closed. No employee connection, catalogue global account or different organization principal is an automatic fallback. The external provider determines the real account identity; the local Dot name is not evidence of an external account.

## Access and live revocation

Use one organization-aware extension of `agents/access.rs::effective_access`:

1. Resolve actual Dot ownership.
2. For an organization owner, resolve current membership from the user's current roles and current selected group bindings.
3. Require membership before applying the existing share access. Explicit gateway administrator recovery remains available for management; execution still uses only principal grants.
4. Preserve existing share semantics: read/write require `can_manage_agents`, respond requires the actual respond or stronger share. `can_manage_dots` authorizes personal Dot lifecycle only; it does not imply organization resource provisioning.

Apply the combined decision to list/detail, draft/save/publish, resources, shares, share-subject search, history/debug/export, Inbox list/detail/events, and staff response routes. The share-subject search must return eligible actual organization members/groups, not an unrestricted installation roster. Respond-only access exposes only the existing minimal item context; credentials and full conversations remain inaccessible.

A member removing the last write share still meets existing last-writer protection. Admin recovery must be visible and tested; membership loss can make all persisted writer shares ineffective even when the share table contains writers.

Before claiming an organization approval, reload the responding user's current groups and current organization membership/share. Do not rely only on a `Viewer` constructed earlier in an SSE stream. Repeat the check in the atomic settlement admission so revocation between lookup and claim cannot authorize a call. Persist actual responding user attribution using the existing resume/audit path.

An organization runtime wrapper must re-read current owner existence, Dot active state, principal active state, exact tool/model/connector grant and selected credential generation before model/tool dispatch, resume and result delivery. Reuse existing tool rules after this authority ceiling. A changed grant or credential cannot be restored by an old `SystemPrincipal` snapshot, saved AgentSpec or pending approval. Already completed external effects cannot be undone; the guarantee covers subsequent dispatch and pending decisions.

Membership revocation removes the member's management/history/response access. It does not transfer ownership or automatically stop every independently authorized organization automation. Dot/principal deactivation, selected credential loss, relevant grant revocation and organization deletion stop or refuse affected work through the existing exact-turn lifecycle paths.

## Memory and context

Generalize the existing Dot-note table/accessors to owned Dot notes associated with the shared typed Dot record. Preserve `Memory`, `MemoryKind`, timestamps, bounded list/search and ownership checks. Organization Dot notes use the Dot's actual organization owner, never a surrogate user.

For knowledge intentionally shared by all Dots of an organization, add explicitly organization-owned memory rows and expose them through the same bounded memory context/CRUD mechanisms. Reading them requires current membership and effective read access on the relevant organization Dot. Dot-specific note changes require current membership and effective write access on that Dot; respond-only access never permits memory browsing. Shared organization-note mutation also needs organization-wide management authority: writing one narrowly shared Dot does not authorize changing knowledge supplied to differently shared Dots. The initial implementation reuses the existing gateway administrator check from organization metadata/group provisioning together with current membership. Administrator recovery may explicitly change membership configuration; it does not automatically permit note browsing. Do not derive this authority from `created_by` or an arbitrary Dot share. A later delegated organization manager would require an explicit approved share-bearing management resource. This is a distinct lifetime/scope, not a personal profile alias. Both scopes require actual typed owner authorization. Organization execution reads its own Dot notes and permitted organization notes; it reads no member preferences, location, private skills or personal memories.

Reuse `remember`, `recall`, `update_memory` and `forget` with an explicit typed memory context when implementing real organization callers. Change the current person-only resolver in that complete slice; never fabricate a user principal to make it pass. Per-action approvals still use the existing AgentSpec rules. User-to-organization knowledge promotion requires an explicit reviewed product flow and is excluded here.

Run slots remain `agent_state` scoped to their actual session. Automatic curation uses the shared metered side-call/job mechanisms when its own contract is implemented; organization ownership does not imply an existing curation service.

## Run ownership, background execution and limits

Organization conversations use existing principal sessions. Pin the actual AgentSpec version and retain current principal grants as the authority ceiling. The requesting/approving user is audit attribution, not conversation owner or execution principal.

Extend existing schedule/webhook records with a neutral, mutually exclusive user/principal owner and typed runtime owner resolution. A principal owner must resolve to the actual organization Dot and organization before dispatch. Existing ordinary and personal user paths keep their owner checks. The organization path uses `RunProfile`/principal tools and returns history through existing agent conversation surfaces.

Reuse mandatory assistant-turn correlation, snapshots, reuse validation, waiting status, startup reconciliation, cancellation and capped trigger request handling. An organization's trigger may reuse only a session of its exact principal and approved context. Missing owner data never creates an ordinary user session. Pending approvals go to current eligible organization responders through the existing Inbox.

Keep existing agent run metering and root-agent budgets. Organisation-wide usage summaries can aggregate existing agent records through actual owner joins; organization-wide budget enforcement is not present and must not be advertised. No new payer engine is required for the first organization run.

For active counts, use the same transactional capacity mechanism as personal Dot creation/reactivation, grouped by typed owner. The default is four active Dots per organization. Add `dots.max_active_per_organization` beside the existing `dots.max_active_per_user` setting and use the same operator settings surface. Lowering a cap blocks future activation rather than inventing a global parallel-run cap. Multiple conversations may execute independently under existing worker and spend limits.

## API and actual-data UI flow

The configuration foundation exposes these organization endpoints:

- `GET /api/v0/organizations`: authorized actual organizations with actual names and effective management rights.
- `POST /api/v0/admin/organizations`: administrator creates metadata and selected existing gateway-group bindings atomically.
- `GET /api/v0/admin/organizations`: administrator inventory with actual selected groups and the existing group catalogue.
- `PUT /api/v0/admin/organizations/{id}`: administrator updates metadata and bindings with the returned `expected_revision`; stale edits return a conflict.
- `GET /api/v0/organizations/{id}`: current member inventory with actual metadata, active usage and readable shared Dots.
- `POST /api/v0/organizations/{id}/dots`: membership plus existing agent-management permission creates the Dot, principal, typed ownership and initial write share in one transaction, subject to capacity.
- `POST /api/v0/organizations/{id}/dots/{dot_id}/active`: current member writer activates or deactivates the actual organization-owned Dot under the independent cap.

Reuse existing `/api/v0/agents/{id}` configuration, grants, shares, conversation and Inbox routes through the combined owner access decision. Extend the Dot inventory/access card with typed owner data where actual organization rows are shown. Do not make personal `/dots/{id}` handlers accept organization IDs without changing their documented authorization contract.

Credential management reuses the identity mechanism and existing integration cards. Current organization connection choices contain only that Dot’s own principal and live connection. Organization-wide credential provisioning and selection remain unavailable. Its response contains selected identity, catalogue key/name/auth, status and nullable verified account label; no secrets or invented provider identity.

Steps one through five describe current configuration and own OAuth management;
runs and knowledge remain adopted implementation requirements:

1. An administrator creates an organization with its actual configured name and selects real gateway groups from the existing group selector. The screen explains that selected groups supply membership and shares still determine access.
2. A qualified member opens that organization, creates a named Dot and uses the existing agent editor/model selector. The actual organization owner is shown.
3. An operator grants the Dot specific resources through existing principal grant controls. Membership grants no execution tools by itself.
4. A current member writer opens **Setup** in the existing agent editor and connects an account for the Dot through supported OAuth, then explicitly saves that connection. The provider controls the account; missing verified labels are shown as unavailable.
5. The member configures existing tool/action rules and shares. Responders appear as eligible actual members or bound groups. The screen shows which effective rights each share permits.
6. A member starts a private principal run. A background trigger is offered only after the owner/resume bridge is implemented. Pending decisions appear in the existing web Inbox; no Slack channel is required.
7. Removing membership removes access; disabling the Dot stops/refuses its work. Missing credentials remain visible and require an explicit reconnect/selection.

The coordinator confirms this walkthrough against seeded real data before UI implementation. All new strings use the six canonical Fluent catalogues. Reuse existing model, resource, permission, connector-status and share labels.

## Adopted foundation contracts

The first organization package includes metadata, immutable Dot ownership,
current membership and the existing private agent configuration surfaces.
Current member writers can manage the Dot’s own OAuth credentials. Organization
execution, memory and triggers remain unavailable until their explicit principal
admission paths are implemented.

The authenticated navigation entry `/organizations` shows the actual membership
inventory or an empty state. Its visibility does not assert membership and is
independent of the personal Dot management flag. Administrator metadata
recovery uses `/admin/organizations` under the existing Access section.

Activation uses `POST /api/v0/organizations/{id}/dots/{dot_id}/active` with
`{active:boolean}` and returns the existing agent view with typed Dot metadata.
It requires current write access, an actual Dot belonging to that organization,
and the existing capacity check in the same transaction. Personal activation
retains its existing route and exact owner check. Neither activation nor
publishing bypasses the temporary organization execution refusal.

Current group mapping and manager/admin flags are loaded with the existing
gateway-group snapshot SQL in the admission `WriteTx`. A temporary Resolver
reuses the existing snapshot builder and actual bootstrap-admin configuration;
it does not change the installation's cached Resolver. Membership, shares and
the mutation or staff claim use this same transaction. The independently
cached skill overlay is not a transactional authorization proof.

Resource selectors and grant writes reuse actual principal grant references
and the manager's grant ceiling. A grant is permission, not evidence that a
tool supports organization execution. No new universal principal-capability
metadata is inferred. Organization-owned account selection and personal-only
tool replacement require their real callers in the later execution package;
the foundation offers no employee or shared-account fallback.

## Implementation phase order and tests

### 1. Typed owner and membership foundation

Own new organization accessors and shared Dot ownership in `aiplane-agents`; coordinator owns the single unreleased schema amendment/checksum/fixture. Runtime owns the combined access resolver; API owns actual organization endpoints and existing agent gates; UI owns settings and owner display.

Test first with actual in-memory SQLite: XOR owner constraint, unknown group/organization rejection, creator deletion preserving organization ownership, personal owner isolation, explicit admin recovery, cross-organization denial, default no membership, current group remapping and member removal. Route tests cover unauthenticated, member, nonmember, manager and respond-only cases.

### 2. Credential ownership and selection

Extend the existing neutral connection owner and personal OAuth mechanism with real organization callers. Test OAuth state binds initiator and target, revoked membership fails before exchange/save, refresh generations cannot resurrect disconnects, account choices cannot cross organizations and missing credentials never use employee/global fallback. Test encrypted storage and actual mocked provider/MCP responses. No generic provider account label is invented.

### 3. Organization run and live authority

Use actual RunProfile/principal session callers and existing editor/test-chat/history. Add current authority validation before each dispatch/resume. Test model/tool/connector removal during a paused and active run, changed identity/generation, principal disablement, deleted organization, responder membership revocation between lookup/claim, private responder exposure and actual root-agent usage. Verify ordinary agents and personal Dots retain their contracts.

### 4. Typed durable memory

Generalize real Dot-note callers and add explicit organization scope. Test no user-profile context, no member token/skill fallback, per-Dot versus shared-organization knowledge isolation, bounded retrieval, CRUD approvals and organization lifecycle cleanup. Cross-organization memory IDs fail without disclosing content. Session state and durable notes remain distinct.

### 5. Background ownership bridge

Extend existing cron/webhook storage/workers/forms after owner/resume contracts are green. Test principal reuse isolation, atomic assistant correlation, waiting/resumed/restarted completion, principal-funded usage, requester deletion, grant/credential loss, current responder checks and exact-turn cancel. Anonymous webhook responses preserve existing non-disclosure behavior.

### 6. Product completion

Coordinator verifies real organization/group/credential metadata, mobile and keyboard flows, history and Inbox in the seeded gateway. Update affected user/admin/API/operator docs, six locales, generated exports and deploy documentation for any runtime knob in the same complete commit. Run architecture and relevant regression suites, then the repository's final verification/review gate.

## Remaining implementation boundaries

- Shared memory write authority uses the existing gateway administrator check from organization metadata/group provisioning together with current membership. There is no existing organization-manager role to claim. Per-Dot write access cannot change all organization notes on its own. Organization runtime reads use explicit organization principal scope; human reads still require membership and relevant Dot read access.
- Existing root-agent budgets and metering remain the initial billing contract; no organization aggregate budget is promised.
- Direct personal-to-organization or other cross-owner communication remains excluded until explicit approval. Internal organization Dot communication can reuse directed consent after the typed organization ownership foundation is implemented and its principal execution callers are covered.
- Personal-to-organization knowledge transfer requires an explicit reviewed privacy flow and is excluded.
- Provider-specific account provisioning, organization discovery from OIDC, automatic employee credential import and additional approval/queue subsystems are excluded.

### Configuration and member search boundaries

The foundation configures organization Dots through the existing agent workspace. Execution remains unavailable until the organization principal bridge is complete. The resource catalogue describes actual grant permissions; it does not claim that every tool supports organization execution. Organization connector tool grants and inventory remain unavailable until the principal runtime bridge is complete; current member writers manage explicitly selected own OAuth credentials through the existing connection panel.

Share-subject suggestions reuse the existing response cap of eight. Organization candidate reads reuse the same user mapper, escaped search and ranking, in transaction pages capped at fifty. Current membership is checked before suggestions are included. A search examines at most one thousand matching candidates; if an actual continuation remains at that boundary, its response carries `truncated=true` and the UI requests a narrower query. The search does not enumerate the user roster or introduce another OIDC mapping mechanism.

## Per-Dot organization OAuth contract

The first organization credential package uses each Dot's own existing
principal. `mcp_connections`, `pending_mcp_oauth` and
`dot_connector_bindings` already represent that credential owner, its initiating
person and the saved selection. This package needs no additional token store
or organization-wide credential principal. Selecting a shared organization
credential requires its separately defined provisioning and inventory caller.

Generalize the existing canonical `/api/v0/dots/{id}/connections` routes by
typed Dot ownership. Other personal Dot endpoints retain their personal
authorization contract. The organization editor uses those same connection
routes and existing integration controls. Organization rows offer only an
explicit principal identity, never the employee's connection. An absent
binding or credential is unavailable; it cannot fall back to the employee or
gateway identity.

Reuse `current_user_policy_in`, `effective_access_in`, `dot_owner_in` and
`organizations::is_member_in` in one `WriteTx`. Organization credential
management requires actual current membership and Write access. Administrator
configuration recovery alone does not grant credential access. Personal
management retains actual personal ownership and `can_manage_dots`.
Inactive Dots remain configurable; active and enabled principal state govern
execution independently.

The existing `authorization_admitted_in` helper supplies the shared proof
before discovery, reservation, callback exchange and conditional credential
save. Selection and local erasure use the same current authority. No SQLite
transaction spans remote HTTP. Recheck membership and generations when the
remote result is committed. Local erasure remains available to a current
authorized member when a connector has been removed or made unavailable.

The connection remains owned by the Dot principal when its creator is deleted.
Deleting an OAuth initiator cancels that person's exact pending authorization
before the initiator foreign key removes its pending state. The cancellation
removes an initial tokenless reservation or clears the authorization deadline
on an existing credential; it does not transfer or erase an already saved
organization credential or selection. Existing owner-keyed refresh locks, cache keys,
catalogue dispatch fences and conditional writes remain authoritative.

Organization execution requires its own selected-connection admission caller.
The existing ordinary principal layer intentionally selects granted shared
resources and cannot establish explicit organization identity. That execution
package must resolve the persisted principal selection through the existing
MCP manager, enforce current principal grants and binding/credential/catalogue
generations, and prove that employee preferences and tokens are never read.

Tests cover member Write access without the personal Dot flag, rejection of
Read/Respond/nonmember and administrator-only recovery, membership loss before
exchange and save, user-mode and cross-organization rejection, inactive
management, generation conflicts, local erasure after catalogue removal,
creator deletion and pending-initiator deletion. Mocked provider responses and
real encrypted storage supply the evidence; account labels remain unavailable
unless a provider-supported response verifies them.

## First executable organization web slice

The first runtime package admits only the existing private draft test chat and
its durable staff responses. Published, pinned, public, embedded and automated
test-suite entry points remain refused until their actual callers establish
organization authority. The synchronous architect test helper is outside this
first admitted entry point.

Pass the authenticated initiating member to the existing draft opener for
admission only. Recheck current membership and Write access in the same write
transaction that creates or continues the private principal session and its
turn. Administrator configuration recovery without membership does not admit
this execution surface. History and staff settlement retain their existing
privacy tiers and fresh membership checks.

Use the existing agent test-chat surface, `Actor::Agent`, its independent
system principal and a private principal-owned run conversation. The initiating
person and responder supply audit attribution; neither becomes the execution
identity. Existing `GrantedToolSource`, agent spec selection, agent budgets,
metering, activity and staff suspension claims remain shared mechanisms.

The current ordinary principal MCP layer selects shared connections, and its
System dispatch branch accepts shared execution facts. The organization
runtime must compose its explicitly selected principal connection and admit
the corresponding owned execution facts. Existing current principal connector
grants and catalogue-generation checks remain required in addition to the
organization binding and credential checks.

Catalogue groups govern human visibility, connection, selection and grant
admission. Organization membership groups do not become runtime roles. Runtime
authority comes from the initial and current explicit principal grants;
changing catalogue groups alone does not revoke a separately granted principal.
Grant removal, disabling the resource or principal, Dot deactivation and
connection or binding changes provide the corresponding runtime fences.

Extend the existing target-aware resource inventory and grant evaluator for
the actual organization principal. A selected own OAuth credential must have
current binding, credential and catalogue generations before it can be offered
or granted. The managing member's current catalogue eligibility and MCP grant
ceiling still apply. A credential selection does not itself grant connector
execution; initial grant admission cannot require the grant it is creating.

The initial `RunToolSource` principal grant set is a ceiling. Before each
organization tool call, intersect it with current principal grants and verify
the actual immutable organization owner, Dot activation and enabled principal.
A fresh tool inventory alone does not prove that a retained tool reference
has current permission.

The current driver refreshes the execution context for personal Dots. Extend
that existing refresh mechanism to a fully initialized organization context
before main-model acquisition and the actual guard, verifier and side-call
models. Directly constructed agent runs and headless dispatch must use the
same context. Remove the profile, direct-headless and resume refusals only
when all their real callers establish that proof.

Personal memory tools require a person actor; do not supply an employee actor
to make them work. Organization knowledge needs its explicit later owner
scope. The first executable web slice must prove that model requests contain
no employee profile and no member token or gateway credential fallback.

State-based SQLite and mocked-upstream tests cover own-principal OAuth,
mid-turn grant removal and deactivation, identity and catalogue changes,
approval persistence across restart, current responder membership, private
principal conversations and Dot-attributed usage. Preserve ordinary principal
and personal Dot execution tests. Background scheduling follows this verified
web slice through the existing workers.
