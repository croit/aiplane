# Cross-organization delegation: personal Dots calling organization Dots

## Status and scope

**Design, not yet implemented.** This document specifies the cross-boundary
delegation that [organization-owned Dots](dots-organizations.md) excludes until
designed: a personal, user-owned Dot delegating a bounded task to an
organization-owned Dot. The motivating case is an organization "service" Dot —
for example a ticket agent — that a user's personal Dots can address to open a
ticket.

The scope decision — whether this package is personal→organization only, or
also internal organization→organization — is open and listed at the end. The
design is written so that personal→organization is the core and
organization→organization is a smaller increment on the same machinery.

Personal same-owner delegation ([dots-communication.md](dots-communication.md))
and organization ownership/execution ([dots-organizations.md](dots-organizations.md))
are the two foundations this composes. Nothing here is a new dispatcher, queue,
approval engine, or message store.

## The requirement

A user-owned personal Dot calls an organization-owned Dot with an explicit,
bounded message. The organization Dot runs as its own system principal — its
own `RunProfile`, grants, selected credential and memory — never adopting the
caller's user credentials, tools or personal memory. It performs the task with
its own authority and returns a bounded answer. The caller's run is charged to
the user; the organization Dot's child run is charged to the organization
principal. Approvals for the organization Dot's own tools settle in the
organization's member-writer Inbox, not the caller's personal Inbox.

## Verified source baseline

| Source | Existing mechanism | Consequence |
|---|---|---|
| `agents/router/local.rs::resolve`, `LocalReceiver` | `Principal(Box<RunProfile>)` and `Personal(Box<PersonalReceiver>)` are both already resolved | The receiver kind is already a two-way enum; a cross-boundary target reuses the principal receiver |
| `agents/router.rs::run_child` | Opens a principal child session via `Owner::Run { principal_id, … }` and `RunProfile` | The organization Dot's child session opens on the existing principal path — no new session opener |
| `aiplane-agents/src/db/child_sessions.rs::LocalChild` | `Principal(RunSession)` and `Personal(PersonalChildSession)` both exist | The child-linkage model already names a principal child; the cross-boundary link is a principal child of a personal parent |
| `aiplane-core/src/server/run_chain.rs::Frame` | Keyed by `principal_id`, no owner type | The chain already holds a mixed-owner tree; `MAX_DEPTH` and cycle rejection apply across the boundary |
| `aiplane-agents/src/db/dot_delegation.rs` | `dot_delegation_edges(caller, receiver, outbound_enabled, inbound_enabled, generation, …)`, same-owner SQL | Generalize the owner join, do not add a table |
| `agents/profile.rs::RunProfile::load` | Principal run profile: active principal, spec, model grant, principal tools | The organization Dot's execution authority is already loaded this way; a `Delegated` purpose extends the organization contract to the published spec without a person's authority |
| `agents/inbox.rs`; member-writer Inbox | Share-based standing, durable suspension/resume | Organization approvals settle here; membership is already checked on every standing/settlement path |
| `server/side_call.rs::Payer`; agent budgets | Principal metering and root-agent budget | The organization child run's payer is its own principal — the existing agent billing contract |

## Shared mechanisms

Reuse, in the order the request touches them:

- `forward_request` and the local dispatch/result machinery (`resolve`,
  `run_child`, `dispatch_result`).
- The directed consent edge (`dot_delegation_edges`), generalized to owner types.
- `RunChain`, `MAX_DEPTH = 3`, cycle rejection, parent call attribution.
- The shared `SpendMeter` tree checkpoint and per-run `Budget`.
- `ChildPause`, `chat_turn_suspensions`, claim/resume, and the Inbox.
- `LocalChild::Principal` for the organization child.
- The organization principal execution path: `Actor::Agent`, `RunProfile`,
  principal grants, the selected principal credential, member-writer Inbox.
- `agents/access.rs::effective_access` (the organization-aware extension) for
  both the consent configuration and live admission.
- The six-language Fluent catalogues.

New shared contracts, landed with tests before the feature uses them:

- A **cross-boundary directed-consent check** that resolves the owner type on
  each end and requires the matching authority per side.
- A **cross-boundary child-link** accessor (record, validate, root-track,
  cancel) for a personal parent → principal child, in the agent layer.
  `session-core` stays owner-agnostic and learns no organization type.

## Data model

One unreleased amendment, in place: `dot_delegation_edges` keeps its shape and
gains owner-type resolution through the existing `dots` table, which already
carries a mutually exclusive `owner_user_id` / `organization_id`.

- A row is still one ordered pair `(caller_dot_id, receiver_dot_id)`, both
  referencing `dots(principal_id)`.
- `outbound_enabled` is the caller side's choice; `inbound_enabled` is the
  receiver side's choice. Same flags, same `generation` semantics as the
  personal case.
- The `authorized` / `authorized_in` check joins each end through `dots` to
  read its owner type and requires: both Dots active, both flags set, generation
  match, and — for a cross-owner row — the caller user's current membership of
  the receiver's organization. A same-owner row is unchanged.
- `set_consent` enforces the authority of the side being edited: the user side
  requires `can_manage_dots` plus actual personal ownership; the organization
  side requires current membership plus Write access on that organization Dot
  (the existing `effective_access` extension). A stale `expected_generation`
  conflicts; enabling a new cross-owner consent requires both Dots active and
  the caller's membership; revocation and unchanged inactive configuration
  remain available.
- Deleting either Dot removes the edge (existing `ON DELETE CASCADE`); the
  nonsecret audit facts are retained.

No new table, no per-package migration. The schema fixture and checksum are
amended once with this change set.

## Execution and data boundary

- The personal caller dispatches `forward_request` with `receiver_dot_id` (the
  organization Dot) and a bounded `message` (≤ 16,000 UTF-8 bytes). Unknown
  fields, an empty message, or over-limit is rejected, as today.
- The receiver is loaded through the **principal** path:
  `RunProfile::load_delegated` for the organization Dot — its published spec
  (a live version is required), its own principal's grants and selected
  credential, under the organization execution contract. It runs as
  `Actor::Agent`. The organization live-turn validation accepts the child
  session, which carries a parent turn, only while its recorded
  cross-boundary link exists.
- Only the explicit bounded message crosses the boundary. **No** personal
  memory, personal workspace, user profile, attachments, or secrets. The first
  slice is text-only, exactly as the personal case.
- The message and the eventual answer are untrusted content: task data through
  the existing task/input boundary, never spliced into trusted system
  instructions. A returned permission directive cannot change authority.
- The caller's selected tools do **not** replace the receiver's; the
  organization Dot uses only its own grants. The caller cannot pass credentials,
  tools, saved permissions, or its system instructions as execution authority.
- The `forward_request` tool's existing **Allow/Ask** policy is the gate on the
  cross-boundary send. An Ask policy makes each send an explicit user decision;
  no new approval subsystem is added.

## Authorization and live recheck

Directed consent, both sides, default deny. For a personal caller A to ask an
organization Dot B:

- A's owner enables `outbound` on the A→B row (`can_manage_dots` + personal
  ownership).
- A current member writer of B's organization enables `inbound` on the A→B row
  (membership + Write on B).
- A→B does not imply B→A. Same-owner personal edges are unaffected.

Recheck current authority at initial dispatch, before every nested delegation,
before child resume, and before returning the child result to the caller. Each
recheck resolves the owner type on both ends and verifies: both Dots active,
both consent flags, edge generation, the caller's current membership of the
receiver's organization, the caller's current `can_manage_dots`/ownership, the
receiver's current principal activation, grants and selected-credential
generation. Revoked consent, membership loss, organization-Dot deactivation, or
credential change prevents continuation and result delivery, not only new
requests.

## Billing and metering

The shared `SpendMeter` keeps one tree checkpoint across the mixed tree. The
personal caller's run is charged to the user. The organization Dot's child run
is charged to **its own principal** — the existing agent budget and metering
contract (`Payer::agent` / root-agent budget). No organization-wide aggregate
budget is introduced or implied; the "principal payer" the organization doc
names as a prerequisite is exactly this existing per-principal metering. A
finite token allowance remains an admission limit on tool-bearing work, not a
monetary ceiling.

## Child linkage, resume, cancellation

The personal caller's child is a `LocalChild::Principal` (an organization
principal session), not a `LocalChild::Personal`. The immutable child link
records: caller personal-Dot principal, receiver organization-Dot principal,
parent assistant turn and tool-call IDs, and the edge generation.

The existing personal-owner-scoped accessors (`root_session_id`,
`active_roots_for_edge`, `validate_child_in`, `personal_parent_in`) filter
`s.principal_id IS NULL` and a single `owner_user_id`. A cross-boundary variant
must track a personal parent whose child is a principal session: the user's
personal Dot remains the root of the tree from the user's side; the organization
principal session is a verified child branch. Cancelling the user's root cancels
the verified linked organization work through the existing exact-turn lifecycle.
Restart continuation validates the recorded link and current cross-boundary
consent before atomically claiming the chain, and never reruns a completed child
effect.

This accessor lives in `aiplane-agents` beside the existing personal child
accessors. `session-core` receives no organization or cross-boundary type.

## Readability boundary

This differs from the personal case and is deliberate. In personal→personal,
the delegated child is the same owner's private session and is owner-readable
through `delegation.root_session_id`. In personal→organization, the child is an
**organization principal session**:

- The caller sees, in their own personal Dot conversation, the `forward_request`
  tool call and its **bounded answer**. That is the caller's visibility.
- The caller has **no read access** to the organization's child principal
  session. Giving a non-member read access to an organization's private
  conversation would break the organization's data boundary.
- The organization child session is readable by current member writers through
  the existing organization history surface, under the existing organization
  access rules.

So the user follows the delegation in their own conversation; the organization's
internal run stays inside the organization's boundary.

## Approvals and settlement

If the organization Dot needs an approval for one of its own tools, its durable
`ChildPause` propagates to the caller's run, and the decision settles in the
**organization's member-writer Inbox**, not the caller's personal Inbox. The
caller's personal Dot waits on the child's actual durable suspension. A parent
delegation approval (the `forward_request` Ask decision, made by the user) and
the organization Dot's own tool approval (made by an organization member writer)
are separate decisions: approving the request does not approve the organization
Dot's tools. The same `ChildPause`, suspension store, claim/resume, and request
IDs serve both; no organization approval queue is added.

## Discovery

An initialized personal Dot offers as receivers the organization Dots for which
it has an enabled `outbound` consent **and** whose organization the user is a
current member of, and which are active. The `forward_request` tool must be
selected and permitted in the caller's tool set; otherwise no receivers are
offered. Discovery reads one membership-scoped batch and never includes
organization memory, transcripts, or secrets. The receiver metadata shares the
existing bounded context budget with the Dot's own memory.

## UI flow

- The personal Dot's **Communication** disclosure
  ([dots-communication.md](dots-communication.md)) extends to list organization
  Dots as configurable peers, visually marked as cross-organization, subject to
  the discovery rule above. The **Can ask** selector offers them; **Accepts
  requests from** shows the organization side's persisted choice read-only.
- The organization Dot's configuration surface (the existing agent editor) gains
  an **Accepts requests from** section listing the personal Dots that are
  granted `inbound`, with the current member writer's own Write access required
  to change it.
- In chat, the caller's `forward_request` request shows the actual receiver name
  and task; the bounded answer returns to the caller's conversation. The
  organization child is not surfaced as a readable thread to the caller.
- Six Fluent catalogues cover the cross-organization marker, the two consent
  directions across a boundary, unavailable/membership failures, and the
  revocation/depth/budget explanations.

## Test plan (state-based)

In-memory SQLite identities (a user with a personal Dot, an organization with a
member writer and an organization service Dot), real registries, mocked
upstreams:

- No edge, only outbound, only inbound, and reverse direction all deny; matching
  cross-boundary consents permit only an active, member-scoped organization
  target.
- The caller and receiver have different tool selections; the organization Dot
  uses only its own tools, memory scope and credential, and never acquires the
  caller's grants or reads personal memory/workspace.
- The explicit text is the child's input; unrelated personal transcript, memory,
  profile and credentials are absent; a malicious task/result instruction cannot
  alter permission state.
- Self-call, A→B→A, and depth four fail before new model work.
- Usage/audit show the user as payer for the personal run and the organization
  principal as payer for the child run, with the full mixed caller chain.
- An organization-Dot tool Ask produces a real durable suspension settled in the
  organization member-writer Inbox, not the user's; one settlement resumes once;
  expiry/deny/cancel settle consistently; restart does not duplicate the effect.
- Caller membership revocation, organization-Dot deactivation, consent
  revocation, or credential change while waiting refuses resume and result
  delivery. The persisted cross-boundary link cannot be forged to a different
  paused turn.
- Readability: the caller sees the tool call and bounded answer in their own
  conversation and receives no read access to the organization child principal
  session.
- API ownership isolation, stale-generation conflict, inactive saved selections,
  and the mobile/keyboard flow use actual resource data.

## Organization→organization (scope note)

Internal organization→organization delegation is a smaller increment on this
same machinery: both ends are organization-owned principals, so the consent
check is membership-plus-Write on both sides, the child is a
`LocalChild::Principal`, settlement is the (same) organization member-writer
Inbox, and both runs are charged to their organization principals. It reuses the
cross-boundary child-link, discovery, and recheck with the owner-type pair
narrowed to (organization, organization). It is available as a follow-on package
once personal→organization is green.

## Confirmed decisions

1. **Scope** — personal→organization only for the first package.
   Organization→organization is a follow-on package on the same machinery
   (see the scope note above).
2. **Discovery membership rule** — a personal Dot may offer an organization Dot
   only when the user is a current member of that organization, in addition to
   the enabled consent.
3. **Readability boundary** — the caller has no read access to the organization
   child principal session; the caller sees only their own `forward_request`
   tool call and its bounded answer.
4. **Principal payer** — the organization child run's metering uses the existing
   per-principal agent budget; no new organization aggregate budget.

## Build order

Following the personal-delegation convention
([dots-communication.md](dots-communication.md) → "Implementation/test plan"):

1. **Foundation** — the generalized cross-boundary consent check and the
   cross-boundary child-link accessor (record, validate), with state-based
   tests. `session-core` stays owner-agnostic.
2. **Dispatch and result** — the cross-boundary dispatch arm (personal caller →
   principal receiver) reusing `run_child` / `open_session`, the organization
   receiver execution, and the bounded result return.
3. **Durable waiting** — resume, root cancellation, and settlement routing to
   the organization member-writer Inbox.
4. **Configuration and discovery** — the communication config API/UI (personal
   "Can ask" across a boundary; organization "Accepts requests from"),
   membership-scoped discovery, and the six-language catalogues.
5. **Completion** — docs, i18n, generated artifacts, and the verification/review
   gate.
