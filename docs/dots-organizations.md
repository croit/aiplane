# Organization-owned Dots

## Status and scope

Adopted implementation design under the maintainer's delegated product decisions. Organization ownership, organization membership, organization credentials and organization memory are proposed capabilities. They are not available by creating a gateway group or naming an existing agent after a company.

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

## Proposed owner and membership data model

### Organization metadata

Add `organizations` in the existing migration set with `id`, actual configured `name`, `description`, `created_by`, `created_at` and `updated_at`. `created_by` records the operator action; its creator's deletion must not delete the organization.

Add `organization_groups(organization_id, gateway_group)` with foreign keys and a unique ordered pair. Membership is the intersection of these selected group names and the person's freshly resolved gateway groups. There is no new user membership roster, OIDC mapping engine or role vocabulary.

Organization creation and membership editing require existing gateway administration rights. Deleting a linked gateway group removes that membership source and immediately prevents access for people with no remaining qualifying group. An organization with no member groups is recoverable through explicit operator administration; it is not silently accessible to every user.

### One typed Dot owner

Adopted shared storage contract: generalize the existing personal Dot ownership record to a `dots` record linked to the existing agent/principal. Keep the existing name and active state, and use mutually exclusive `owner_user_id` and `organization_id` foreign keys with a CHECK requiring exactly one owner. Expose `DotOwner::User(id) | Organization(id)` in `aiplane-agents`.

This is the adopted amendment, not a claim about current schema. It avoids separate personal and organization ownership tables that could both claim the same principal. Update existing personal accessors and their actual callers in the same vertical slice. Do not add a compatibility view or accept both owners. Personal owner APIs retain user-only authorization.

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

## Proposed API and actual-data UI flow

New organization endpoints are needed because there is no organization entity API:

- `GET /api/v0/organizations`: authorized actual organizations with actual names and effective management rights.
- `POST /api/v0/admin/organizations`: administrator creates metadata and selected existing gateway-group bindings atomically.
- `PUT /api/v0/admin/organizations/{id}`: administrator updates metadata and bindings with the existing CAS/revision convention where supported; introduce a concrete revision only if required by the actual editor.
- `POST /api/v0/organizations/{id}/dots`: membership plus existing agent-management permission creates the Dot, principal, typed ownership and initial write share in one transaction, subject to capacity.

Reuse existing `/api/v0/agents/{id}` configuration, grants, shares, conversation and Inbox routes through the combined owner access decision. Extend the Dot inventory/access card with typed owner data where actual organization rows are shown. Do not make personal `/dots/{id}` handlers accept organization IDs without changing their documented authorization contract.

Credential endpoints reuse the identity proposal and existing integration cards. Organization connection choices contain only actual permitted Dot/organization principals and their live connections. Return selected identity, catalogue key/name/auth, status and nullable verified account label; no secrets or invented provider identity.

Walkthrough:

1. An administrator creates an organization with its actual configured name and selects real gateway groups from the existing group selector. The screen explains that selected groups supply membership and shares still determine access.
2. A qualified member opens that organization, creates a named Dot and uses the existing agent editor/model selector. The actual organization owner is shown.
3. An operator grants the Dot specific resources through existing principal grant controls. Membership grants no execution tools by itself.
4. The member connects an account for the Dot through supported OAuth or selects an existing permitted organization credential. The provider controls the account; missing verified labels are shown as unavailable.
5. The member configures existing tool/action rules and shares. Responders appear as eligible actual members or bound groups. The screen shows which effective rights each share permits.
6. A member starts a private principal run. A background trigger is offered only after the owner/resume bridge is implemented. Pending decisions appear in the existing web Inbox; no Slack channel is required.
7. Removing membership removes access; disabling the Dot stops/refuses its work. Missing credentials remain visible and require an explicit reconnect/selection.

The coordinator confirms this walkthrough against seeded real data before UI implementation. All new strings use the six canonical Fluent catalogues. Reuse existing model, resource, permission, connector-status and share labels.

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
