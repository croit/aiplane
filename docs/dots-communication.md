# Internal communication between Dots

## Status and requirement

Adopted proposal for upcoming implementation. The user explicitly requires named Bots/Dots to communicate with one another under permissions. Personal Dot communication is not currently connected to the local agent dispatcher. This proposal provides bounded request/reply delegation with durable pauses using existing mechanisms; it does not claim an implemented chat channel or asynchronous mailbox.

Initial scope: personal Dots belonging to the same user. Organization Dots may communicate within the same organization only after typed organization ownership and approval/session boundaries exist. Cross-user and cross-organization communication requires a separately explicit sharing/data-exposure design; ordinary group membership does not grant it implicitly.

## Verified mechanisms and gaps

| Source | Fact |
|---|---|
| `aiplane-runtime/src/agents/router.rs` | `forward_request` dispatches `RouteTarget::Agent` locally through `run_child`, loading the receiver's published `RunProfile`, own principal, budget and finish contract. It opens a child principal-owned session and appends a `RunChain` frame. |
| `aiplane-runtime/src/agents/spec/model.rs`; `spec/route_kinds.rs` | Route targets distinguish local Agent, Human, external A2A and Loop. Routes render a task and bound state values; the child receives that task, not an automatic transcript copy. The current source uses `RouteTarget::Agent`, not a named `LocalRouteTarget` type. |
| `aiplane-core/src/server/run_chain.rs` | `MAX_DEPTH = 3` includes the root. `RunChain::enter` rejects both excessive depth and an already-present principal, including indirect cycles. Frames retain principal/version and parent tool-call attribution. |
| `aiplane-runtime/src/suspend.rs`; `router.rs::dispatch_result` | `ChildPause` carries the child's assistant turn and expiry. The parent waits on the child's actual durable suspension; completed results are returned as data and dispatched/finished audit events are recorded. |
| `openai_driver/resume.rs::check_child`; `agents/resume.rs` | Child pauses are checked against principal run-session parent linkage and an actual suspension. Resume claims the pause chain and distinguishes staff/participant authorization. This linkage is currently principal-session specific. |
| `agents/access.rs::effective_access` | Admin has write access. Other people need existing shares; read/write additionally require agent-management permission, while respond can survive without it. This is management/decision access, not a generic right for any Dot to invoke another. |
| `server/grant_holding.rs`; `principal.rs` | `a2a_caller` refers to permission for a remote principal to call a particular agent; granting requires target write access. `a2a_agent` refers to an external card URL and is administrator-controlled. These grants are not directional local Dot communication rules. |
| `agents/a2a.rs`; `agents/a2a_client/mod.rs` | Remote A2A exposes bearer-authenticated agent cards/JSON-RPC and checks external-target grants. Local communication does not need those HTTP routes, system tokens or public cards. |
| `aiplane-agents/src/db/{a2a_contexts,agent_a2a_tasks}.rs` | Remote contexts carry caller/token facts; remote pending tasks carry card URL/task/context IDs. They are not a general local mailbox. |
| `persona/personal_dot.rs`; `server/headless.rs` | Personal Dot execution uses the user-owned private session and user authority narrowed by the Dot's rules, with separate Dot memories. It must not be routed through principal-only execution that loses this ceiling. |

The actual local dispatcher is reusable, but its published-principal profile loader and parent-child/resume accessors require an explicit personal-Dot branch. Merely selecting a Dot principal as a route target would drop the current personal execution model. Existing public A2A permission fields cannot safely stand in for the new local delegation boundary.

## Shared mechanisms

Reuse `forward_request` and its local dispatch/result machinery; typed route/task rendering; `RunChain`, budgets and finish contracts; existing user/private or principal-owned sessions; `ChildPause`, suspensions, claim/resume and Inbox; existing live tool authorization; model catalogue/pool access; audit and usage recording.

Extract a single typed local dispatch target resolver used by ordinary local agent routes and Dot calls. It resolves either the existing principal profile or a verified personal Dot profile and carries the correct execution actor. Keep ownership lookup in `aiplane-agents`, execution in runtime and session-core owner-agnostic. Extend existing parent-child linkage accessors to recognize both principal run sessions and personal Dot sessions; do not add a separate resume engine.

Add one explicit directed Dot delegation edge, checked by this resolver and the live dispatch tool. This is a relationship permission, not a new tool policy or A2A protocol. The communication capability must also be selected in the caller's existing tools and subject to its Allow/Ask policy; edge permission alone cannot expose a hidden tool.

## Authorization and minimum persisted data

Default deny in both directions. For caller A to ask receiver B, A must explicitly allow outgoing delegation to B, and B must explicitly accept incoming delegation from A. Same-owner personal Dots can be configured by that owner; no edge is created merely because both exist. A→B does not imply B→A.

Use one row per ordered pair in `dot_delegation_edges(caller_dot_id, receiver_dot_id, outbound_enabled, inbound_enabled, generation, updated_at)`. Both choices are separately presented on their respective Dot, default false, and require current manage/owner access to the corresponding endpoint. Generation changes on either decision. Enforce actual ownership/organization equality at save and invocation, not solely a client filter. Delete identity edges when either Dot is deleted while retaining nonsecret audit facts. Amend the one unreleased Dot migration and fixtures once; no per-package migration.

Record the authorized edge ID/generation, caller/receiver IDs, parent assistant turn/tool-call IDs and the receiver's child turn with existing dispatch/suspension context. Add parent-turn and caller-Dot linkage to the existing personal Dot session association where needed; use a shared agent-layer accessor for verified child relationships. Do not make session-core know a Dot. No new queue or message store is needed: the child user turn holds the bounded explicit task and the existing result/suspension stores hold outcome and wait state.

Check authorization at initial dispatch, before every nested delegation call, before child resume and before returning the child result to the caller. Recheck current ownership, both active states, selected communication tool, edge generation, current roles/grants and receiver execution policy. Revoked consent or changed ownership prevents continuation and result delivery, not just new requests. Existing child external calls retain their own live tool/credential/action revalidation; caller consent cannot bypass them.

## Receiver execution and data boundary

A personal receiver runs as its own personal Dot context with its own user owner and tools, memory scope and approval rules. Same owner does not imply identical selected tools. It may use its own authorized memories to answer the explicit task, but the sender receives only its deliberate bounded answer, not direct memory access. The caller cannot pass credentials, tools, saved permissions or its system instructions as execution authority.

An organization receiver runs as its own granted principal and explicit organization/Dot credentials, without fallback to an initiating employee. Organization tool/model/approval authority must be established before that phase executes. Agent management shares and organization membership narrow management/response access; they do not automatically authorize delegation.

Inputs contain an explicit bounded message and, only when deliberately selected and authorized, attachment references. No automatic transcript, memory snapshot, workspace mount, profile data or secrets are shared. Each attachment must be owned/visible to the request owner and safely readable by the receiver's scope; do not forward arbitrary object-storage URLs or unchecked IDs. Start the first slice with text only, since attachment authorization across child sessions must have a real verified accessor before it is offered.

Treat the message and eventual answer as untrusted content. Frame them as task data through the existing task/input boundary; do not splice them into trusted system instructions or accept returned permission directives. Return the existing finish/outcome data with a bounded answer/error/wait state. Use existing tool-result budgets and content protections. Do not return the child's transcript or journal. Attachment output requires the same explicit access check and no implicit copying of the receiver's workspace.

Personal Dots currently have no independent saved model choice. The first call uses the initiating turn's actual selected model only after checking that it is usable by the receiver's owner and applying existing model/pool limits; refuse if unavailable rather than invent a model. Principal receivers retain the model in their actual validated profile. Model selection does not grant tools or select a credential.

## Calls, budgets and durable waiting

Extend the existing local `forward_request` path with a typed Dot target and explicit task input. Existing agent route callers retain their current schema/route behavior; the actual personal-Dot tool source offers its authorized targets through the same local dispatcher. Do not expose public A2A endpoints or mint tokens to make an internal call.

Enter the receiver frame in the caller's existing chain before opening child work. Preserve `MAX_DEPTH = 3`, cycle rejection, parent call attribution and the existing receiver budget capped by the remaining parent budget. Enforce the existing owner/principal spend limits and tool-result/time/token bounds. Limit repeated sibling dispatches through existing turn/round/tool budgets; depth checks alone do not bound a flat fan-out. The implementation must prove those budgets are shared/capped correctly before claiming a total delegation-tree spend ceiling.

If the child needs approval, propagate its durable `ChildPause` to the parent with the actual child expiry. The receiving authority settles that child action, then the parent continues with the minimal result. A parent delegation approval and a child external-write approval are different decisions: approving the request does not approve the receiver's tools. The same-owner first slice uses the owner's existing Inbox; organization settlement uses its explicit authorized responders when implemented.

Stop/cancel must settle the linked parent/child chain consistently using the shared lifecycle. Repeated resume claims cannot duplicate an already-created child or rerun its completed side effects. Deactivation pauses new work and continuation; revocation must produce an actionable refusal without leaving an orphan wait. Restart handling must reconcile recorded child linkage using existing suspension/startup mechanisms, not assume a continuously running process.

Audit dispatched/finished/denied/revoked events with caller, receiver, parent/child turn, edge generation and outcome; omit task content and secrets from activity details. Usage retains the actual payer and source through the existing run chain. Personal same-owner delegation is charged to that user, with Dot attribution in the chain; no separate per-Dot budget or usage dashboard is claimed. Organization billing ownership needs the existing principal payer resolved explicitly before that phase.

## Actual-data web flow

Extend each existing Dot card with **Communication** using actual owner-scoped Dot inventory. Show separate **Can ask** and **Accepts requests from** selectors, excluding itself and unavailable cross-owner resources. Default selections are empty. Rows use returned Dot IDs/names/active state; inactive saved targets remain visible and unavailable instead of being silently removed. Save through the normal owner-scoped error/reload pattern and audit the consent change.

Example using real created names: configure Harald's outgoing selection to an existing owned Dot and configure that receiver's incoming selection to Harald. The interface shows whether both consents are present, derived from persisted data. It explains that the receiver uses its own tools and rules and that only the explicit task is sent. The owner can remove either consent; pending work under that edge cannot resume or deliver a result afterward. No fabricated target names, introductions, skills or capability descriptions appear.

Use the existing tools selection and approval controls for the communication tool itself. In chat, its request includes the actual receiver name and task; pending child approvals appear through the existing Inbox with caller/receiver attribution and conversation links authorized for that viewer. Activity contains route/result metadata rather than an implicit shared chatroom. Six Fluent catalogues cover selectors, consent status, unavailable targets, revocation/depth/budget failures and explanations. A coordinator checks desktop/mobile and approval/revocation paths in the real gateway before completion.

## Implementation/test plan and constraints

First establish the shared typed local resolver and personal/principal child-link accessor with real callers/tests. Then integrate directed consent and live dispatch, personal receiver execution, durable resume/cancellation and the existing access/communication UI. Organization communication follows the identity/ownership slice. Shared schema, protocol, catalogs and generated files each have one writer; no duplicate dispatcher, queue or approval store.

State-based tests use actual in-memory SQLite identities/sessions, registries and mocked model upstreams:

- No edges, only outbound, only inbound and reverse direction all deny; matching directed consents permit only same-owner active targets. A tool not selected/granted cannot call despite both consents.
- Caller and receiver have different tool selections. Receiver uses only its own tools/memory scope/credential and never acquires caller grants or raw private data.
- Explicit text is the child's input, unrelated transcript/memory/credentials are absent, and malicious task/result instructions cannot alter permission state. Attachment IDs are rejected until an authorized transfer is implemented.
- Self-call, A→B→A and depth four fail before new model work; sibling requests obey finite existing budgets; usage/audit show correct payer and full caller chain.
- Child Ask produces real durable parent/child suspension; independent child tool approval is required, one settlement resumes once, expiry/deny/cancel settle consistently, and restart does not duplicate side effects.
- Either consent revoked, target disabled/deleted, ownership changed or tool access revoked while waiting refuses resume/result delivery. Persisted linkage cannot be forged to a different paused turn.
- Organization phase refuses different organizations, removed members, ungranted receivers and user-credential/memory fallback. Shares alone never create communication consent.
- API ownership isolation, stale-generation conflict, inactive saved selections and accessible mobile/keyboard flow use actual resource data.

Current source gaps are material: ordinary local dispatch loads published principal profiles; personal Dot child linkage and staff/participant resume are not yet unified; available user Dot tools do not expose communication; direction-specific consent does not exist; a tree-wide budget guarantee must be checked against actual remaining-budget accounting. These prerequisites are implementation work, not supported behavior. The existing remote A2A context/task records are sufficient for their remote protocol but should not be overloaded with invented local card URLs or tokens.
