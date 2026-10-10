# AIplane Dots: product and architecture specification

Status: implementation in progress; personal Dot ownership, active lifecycle,
the first web management page, manually managed Dot memory, recent activity,
owner-scoped memory/activity APIs, and private Dot chat sessions are implemented.

The current implementation branch has the personal-Dot owner and active
state table, transactional active-slot enforcement, and session-authenticated
`GET`/`POST /api/v0/dots` plus `PUT /api/v0/dots/{id}/active` endpoints. The
active cap is an operator setting at `/admin/settings` → Access, defaulting to
four. The `/dots` page lists, creates, activates and deactivates personal Dots.
Lifecycle endpoints currently reuse the existing `can_manage_agents` group
permission. Each Dot card has a lazily loaded memory panel with separate
preference, project and fact entries. Owners can add, edit and delete those
notes; they remain separate from user-wide memory. A second lazily loaded
panel shows the Dot's recent audited events and can page through older ones.
Starting a Dot chat adds a session-start event without copying conversation
text. Personal Dot tool decisions and outcomes appear in the same feed with
the acting user and Dot attribution; their activity detail contains the tool,
decision and status, but not arguments or tool results.
Dot memory has a separate store and owner-scoped API, but automatic memory
curation is not implemented. A personal Dot can start a private chat that
reads bounded Dot memory and the user's existing profile context. Its chat
exposes only the existing memory tools when the user's memory capability is
granted and enabled; external and MCP tools remain unavailable until action
policy is implemented. There is no background execution yet.

| Endpoint | Behavior |
|---|---|
| `GET /api/v0/dots` | Lists only the signed-in user's personal Dots, including inactive ones, and returns the active limit. |
| `POST /api/v0/dots` | Creates a named personal Dot if the user has an active slot. |
| `PUT /api/v0/dots/{id}/active` | Activates or deactivates an owned Dot; activation rechecks capacity transactionally. |
| `GET /api/v0/dots/{id}/memories` | Lists the owned Dot's memories. |
| `POST /api/v0/dots/{id}/memories` | Creates or reclassifies a Dot memory (`preference`, `project`, `fact`). |
| `PUT /api/v0/dots/{id}/memories/{memory_id}` | Updates an owned Dot memory. |
| `DELETE /api/v0/dots/{id}/memories/{memory_id}` | Deletes an owned Dot memory. |
| `GET /api/v0/dots/{id}/activity` | Lists the owned Dot's audited activity, newest first; accepts `cursor` and `limit`. |
| `POST /api/v0/dots/{id}/sessions` | Creates a private chat session owned by the user and associated with the active Dot. |

Lifecycle routes require `can_manage_agents` today. Creation and active-state
changes are recorded through the existing agent activity chain. The Dot
principal starts without grants; personal user grants are not yet available to
its run path. Lowering the configured active cap does not deactivate existing
Dots; creation and reactivation remain blocked until the owner is below the
new cap. Dot memories are separate from `user_memories`, reuse its kinds and
duplicate handling, and can be managed under `/api/v0/dots/{id}/memories`.
a Dot chat session reads bounded Dot memory and the user's existing profile
context. It exposes only `remember`, `recall`, `update_memory` and `forget`,
subject to the user's existing memory grant and setting. Writes go to that
Dot's own store; recall returns both Dot and user memories with explicit
scopes, and user-wide memory is read-only from a Dot chat. Other tools remain
unavailable until action policy is implemented. Deactivating
a Dot preserves its existing sessions but blocks new sessions and new turns;
existing pending Dot turns remain paused while the Dot is inactive. The `/dots`
page displays active usage against the operator limit, creates a personal Dot
with a friendly name, starts a private chat, and lets the owner deactivate or
reactivate it. A deactivated Dot stays in the list and can be reactivated when
a slot is available. The page does not yet offer permission rules, connector
selection, background run setup or organisation-owned Dots.

## Product goal

AIplane Dots are named, persistent AI coworkers that a person or an
organisation can reach through AIplane's web interface. A Dot keeps useful
context across runs, can work from chat, schedules and incoming events, and
uses only the identities, connectors and actions its owner has authorised.
The implementation uses AIplane's own runtime and configured model
upstreams. It does not install or depend on OpenDots, CopilotKit Intelligence,
AG-UI, OpenBot or OpenAI Dots.

The product adopts the user-facing capabilities described by OpenAI Dots and
OpenDots, not their code or hosted services. OpenAI describes scheduled work,
read-only background research, long-term preferences and action review. The
OpenDots repository describes a persistent agent workspace, per-Dot
connections and approvals, and explicitly notes that its schedules are not a
complete goal or event-trigger system. These references inform the product
requirements; they do not determine AIplane's implementation.

## Confirmed requirements

### Dot ownership and identity

- A personal Dot belongs to one user, has its own display name, and runs with
  that user's effective permissions.
- A personal Dot may use a connector with the user's connected identity or a
  connector identity attached specifically to that Dot. The user chooses per
  connector.
- An organisation Dot belongs to an organisation, has its own system identity,
  and must use an organisation/Dot-owned connector identity. It must not
  silently inherit an employee's identity.
- The Dot name is the named agent identity shown to people. For a personal Dot,
  the acting user remains the security principal for permission checks; the
  activity record also identifies the Dot.
- Users may create Dots when the operator enables that capability. The
  operator controls the maximum number of active personal Dots per user.
- The initial default active-Dot limit is four. Deactivating a Dot frees an
  active slot; the requirement limits active Dots, not the historical number
  of disabled Dots.
- Deleting a user removes that user's personal Dot principals, definitions,
  memories, grants and tokens through the existing principal cascade.

### Work and channels

- The initial user interface is AIplane's web application only. Slack is out
  of scope. Voice and other channels are later work unless separately
  approved.
- Dots are reachable when idle and can continue work through scheduled or
  event-triggered runs. "Always on" does not require a model process to run
  continuously.
- Events should reuse AIplane's existing webhook and tool/MCP mechanisms. A
  ticket-monitoring example is handled through the configured ticket-system
  MCP; it does not justify a Gira/Jira-specific integration.
- Dots use AIplane's configured model catalogue and existing upstream routing.
- Existing sandbox and browser capabilities should be reused or extended to
  give a Dot a suitably isolated computer. The browser extension that acts in
  a user's live browser is not sufficient for unattended work.

### Permissions and actions

- A personal Dot's effective permissions are the intersection of the user's
  effective permissions and the Dot's own allowed tools/actions. The Dot may
  narrow user rights but never expand them.
- An organisation Dot uses its own principal and explicit grants.
- Owners choose connectors and configure rules by tool and action.
- Actions outside read-only research and chat require an explicit policy.
  External writes and critical actions must be reviewable and can require a
  human approval before execution.
- Approval checks are enforced in the gateway/tool path, not only in prompts
  or the browser.
- The activity record must show the acting user (for personal Dots), the Dot,
  the selected connector identity, action, decision and outcome.

### Memory and Dot workspace

- A personal Dot can use both its own memory and the user's general knowledge
  and preferences. Organisation Dots must not acquire an employee's personal
  memories by default.
- Memory persists across runs. A background process should consolidate useful
  user knowledge and Dot-specific lessons, including pitfalls, so they can
  prevent repeated mistakes.
- Each Dot has a persistent, isolated data area that it can reuse across runs
  for project work and its own structured information.
- The implementation should reuse user memories, document/canvas storage,
  agent state, and existing tool APIs where they fit. It must not expose
  arbitrary application database access to a model.

## Existing mechanisms to reuse

| Need | Existing AIplane mechanism | Gap to resolve |
|---|---|---|
| Agent definition and display name | `agents`, `AgentSpec`, drafts and published versions | Represent Dot ownership and personal-vs-organisation execution mode |
| Identity and organization grants | system principals, `principal_grants`, agent shares | Personal Dot must act with the user's authority while retaining Dot attribution |
| Chat, runs and streaming | `chat_sessions`, `chat_turns`, `SessionDriver`, JSON-SSE | Associate a personal Dot with user-owned sessions without turning it into a second chat engine |
| Durable pause and approval | `chat_turn_suspensions`, `agents::approval`, Inbox | Interactive chat can pause and resume a tool call; headless schedule/webhook and `/v1` turns cannot currently suspend, so approval-required actions must fail closed there until those run paths gain a durable approval continuation |
| Schedules | `scheduled_actions` and scheduled worker | Current schedules are user-owned; support Dot-owned, agent-context runs |
| Event triggers | Webhooks and headless execution | Current webhooks are user-owned; support Dot identity, event policy and activity correlation |
| MCP catalogue and execution | connector catalog, `McpConnectionManager`, `UserMcpLayer` | Personal delegated and Dot-owned credentials; organisation/Dot OAuth identity |
| Per-user memory | `user_memories`, memory tools, `personal_dot_memories` | Runtime reads/writes and background curation for isolated Dot memory |
| Agent conversation state | `agent_state` | Session-scoped today; Dot workspace must persist independently across sessions |
| Browser and code execution | sandbox-runner, sandbox browser tools, browser-control | Persistent isolated computer/profile/storage per Dot, suitable for background runs |
| Operator settings | declarative settings API and SPA | Dot creation permission and active-Dot limit (default 4); define organization scope |
| Usage controls | user/agent spend and usage limits | Attribute background usage to its owner and Dot; no separate parallel-run cap is requested |

The connector boundary is especially important. Current personal MCP
connections and OAuth tokens are user-keyed. A system-principal run only sees
shared-identity connectors; it deliberately does not read a person's OAuth
connection. Current `agent`-scope connectors accept `none` or `static_bearer`,
not an independent OAuth login per Dot. Personal delegation and
Dot-/organisation-owned OAuth therefore require a reviewed extension to the
existing connector/credential path.

The existing group model is not an organization owner model. `gateway_groups`
map identity-provider claims to access-control groups and resource grants;
they do not own agents, connector credentials or data. An organization Dot
therefore needs an explicit tenant/resource owner model, not a group name on
`personal_dots`.

The current runtime makes the required execution bridge concrete. `Actor` is
a closed choice between `Person` and `Agent`; `TurnPolicy` then chooses either
ordinary chat (user RBAC, selected chat tools) or an agent run (principal
grants and the agent spec's narrower tool resources). A personal Dot needs a
third, explicit policy: retain the user's `Principal` and user-owned session
for identity, billing and private chat, while also carrying the Dot's
`AgentRun` for its spec, memory, activity chain and tool/action allowlist. Its
tools and model access must be the intersection of those two sources. Reusing
the `Agent` policy alone would drop the user's authority; reusing `Chat` alone
would drop Dot policy and attribution.

The current sandbox does not provide the Dot computer's persistence. Its
`SandboxLease` lives in a turn's `ToolContext` and is released at turn end; the
runner's lease table has a configured TTL and capacity cap. Files can be
staged across turns through the chat attachment store, but that store is
conversation-scoped. Neither provides a Dot-owned browser profile or
filesystem across separate runs. Dot workspace files need a Dot-owned storage
key and access checks before sandbox staging; the runtime should not gain a
shared-volume mount.

The existing MCP manager has two separate overlays: `layer_for_user` resolves
the user's allowed per-user OAuth connections plus allowed global connectors;
`layer_for_principal` resolves only granted connectors with a shared gateway
identity. The Dot identity choice therefore belongs in an explicit
owner-and-Dot-aware resolver. It must filter by both user grants and Dot
policy, and apply the selected identity for each connector. Exposing both
overlays wholesale and relying on the prompt to choose would exceed the
confirmed permission boundary.

The always-on mechanisms also have a specific gap. `scheduled_actions` and
`webhooks` are user-owned today. Their headless workers open user-owned chat
sessions and drive `Actor::Person`; a schedule has a broad tools-on/off switch,
while webhooks default tools off and carry the request payload as untrusted
input. A Dot-owned schedule or event trigger must keep the user as session
owner while carrying Dot policy and attribution into the run. It must apply
the per-tool/action rules instead of treating `tools_enabled` as sufficient
approval for external writes. Webhook payloads retain their untrusted-input
framing.

Memory has the same split in the current code. `user_memories` is keyed only
by `user_id`; the four existing memory tools use the acting user's identity
and personal Dot context to route writes into `personal_dot_memories`. Dot
chats expose them only when the user's existing memory capability is granted
and enabled. `recall` combines both scopes and marks each result; correction
and deletion operate only on Dot-scoped memories from a Dot chat. The driver
injects the user's preferences only when the shared `recall` tool is granted
and enabled. Agent `agent_state` is a different store keyed by
conversation session and is used for typed orchestration slots, so it cannot
serve as persistent Dot memory. A personal Dot turn must get user-profile
memory under the user's existing memory grant and switch, plus a distinct
Dot-owned memory scope for lessons and project context. The default write
target must not silently turn every Dot lesson into a user-wide preference.

## Proposed architecture

```mermaid
flowchart TD
  UI[AIplane web: Dots, chat, activity, approvals, workspace]
  API[/api/v0 JSON API]
  Domain[Dot ownership and policy]
  Runtime[Existing agent run and chat runtime]
  MCP[Existing MCP catalog and connection manager]
  Jobs[Existing scheduler and webhook workers]
  Memory[User memories plus Dot memory]
  Computer[Sandbox runner and isolated Dot workspace]
  Audit[Existing agent activity log]
  UI --> API --> Domain --> Runtime
  Jobs --> Domain
  Runtime --> MCP
  Runtime --> Memory
  Runtime --> Computer
  Runtime --> Audit
```

The Dot is a user-facing mode of AIplane's existing agent, not a replacement
for the agent runner. The execution context is explicit:

- `PersonalDot { user_id, dot_id }`: tool access is resolved from the user's
  grants and connector identities, then narrowed by the Dot's own policy.
- `OrganizationDot { organization_id, principal_id }`: tool access is resolved
  only from the Dot principal's grants and its own connector identities.

The concrete Rust type and schema are implementation choices. They must keep
the crate dependency direction intact and must not make `session-core` aware
of Dot ownership.

The personal-Dot slice stores the user-facing name and active state in a
`personal_dots` row keyed by the existing agent principal. User-facing names
are unique per owner, while the system principal retains a generated unique
internal name; two users can therefore each name a Dot "Harald". Organization
ownership still needs its own design because AIplane has no organization
entity in the current data model.

## Identity and authorization flow

```text
request from signed-in user
  -> load Dot and verify owner/share access
  -> establish explicit Dot execution context
  -> resolve owner permissions and chosen connector identity
  -> intersect owner permissions with Dot tool/action policy
  -> apply hard safety rules and approval policy
  -> run the existing tool loop
  -> append activity as {actor user, Dot, connector identity, action, result}
```

For personal Dots, user authority is resolved at execution time so a revoked
user grant or disconnected connector takes effect without republishing the
Dot. For organisation Dots, principal grants remain explicit. A Dot cannot
use its model-visible instructions to change these decisions.

## Persistence concepts

These are logical responsibilities; table names and exact encoding are still
subject to migration design review.

- **Dot metadata:** existing agent definition plus owner type/id, active state,
  execution mode and Dot-specific policy.
- **Connector identity selection:** per-Dot connector choice of user identity,
  Dot-owned identity or organisation-owned identity. Credentials remain
  encrypted and unavailable to model context.
- **User memory:** existing user-wide memory, readable by the user's personal
  Dots only under the user's configured permission.
- **Dot memory:** separate scope for lessons and context owned by that Dot.
- **Dot workspace:** persistent Dot-owned project artifacts and structured
  data, separate from chat transcript and short-lived `agent_state`.
- **Work item/run correlation:** schedule or event source, Dot, session, status,
  attempts, decisions and outcomes must be traceable through current runs and
  activity records. Avoid a new task table unless existing schedule/webhook
  records cannot represent a confirmed lifecycle requirement.

## Action policy

Policy is configured by a Dot owner and evaluated for every tool call. It has
three outcomes: allow, require approval, or deny. Resolution must include the
owner's authority, Dot policy, connector/tool policy and mandatory platform
rules. The most restrictive applicable result wins. Approval is durable and
uses the existing suspension and inbox flow.

Interactive chat already persists a suspension in `chat_turn_suspensions`,
releases its worker and resumes the same tool call after a decision. The
headless scheduler and webhook workers build turns with suspension unavailable;
the `/v1` path does the same. A tool that requires a decision refuses to run
when nobody can resume it. Dot policy must preserve that fail-closed behavior:
do not execute an approval-required action merely because a schedule enables
tools or a webhook starts a run. Background Dot actions need a durable
approval owner and continuation path before they can wait for approval.

Exact built-in action categories, whether creating a draft is itself a write,
and which organization members may approve are open design decisions. Until
those are decided, the implementation must not silently enable external
write actions.

## Background work and memory curation

AIplane already has scheduled runs and webhooks. The first Dot implementation
should reuse those paths and introduce only the ownership/context bridge they
lack. MCP calls may be used as event-driven work inputs where the connected
system supports them; the Dot feature itself does not implement ticket-system
specific code.

OpenAI publicly describes background research as read-only and separately
describes scheduled tasks. OpenDots documents scheduled turns and says its
schedules are not a complete goal/event system. Neither source exposes a
general-purpose internal trigger implementation. AIplane will therefore use
its own scheduler and webhooks, with polling frequency, connector event
delivery and retry policy defined per supported deployment capability.

Memory curation should operate on existing conversations, user memory and
Dot memory through a bounded background run using the configured model
upstreams. It must not become model training. The curation cadence, evidence
retention, approval behavior and conflict resolution remain open decisions.

## User experience

The initial channel is the web SPA:

1. A user creates a named personal Dot when the operator allows it and an
   active slot is available.
2. The user chooses model, tools, MCP connector identity and action rules.
3. The user chats with the Dot, starts or edits ongoing work, and reviews
   activity and pending approvals.
4. The user can inspect and manage user-wide memory, Dot memory and the Dot's
   workspace.
5. The user can deactivate a Dot to free an active slot.
6. An authorized organization manager creates an organization Dot, gives it
   its own identity and grants only the required resources.

UI implementation requires a reviewed mockup/walkthrough before Svelte
changes, per the repository working agreement.

### First web slice walkthrough for review

The management surface for the lifecycle API above uses
only fields currently returned by that API:

```text
Sidebar:  Dots

My Dots                                         1 of 4 active
Named assistants available to you               [Create Dot]

Name          Status       Actions
Harald        Active       [Deactivate]

Create Dot dialog
Name          [Harald________________]
Display name  [Harald________________]
Description   [Optional________________________]
                                      [Cancel] [Create Dot]
```

The count comes from `active` rows and `active_limit`; name, display name,
description and active state come from each returned Dot. The view does not
invent a run status, last activity, or capabilities before those are backed by
the execution and activity APIs. Creation validates the name server-side. The
active row action calls the state endpoint and the count updates from a fresh
list response. The personal execution slice adds a conversation action to each
active Dot:

```text
Name          Status       Actions
Harald        Active       [Chat] [Deactivate] [Memory] [Activity]
```

Selecting Chat creates a private session owned by the signed-in user, associates
it with Harald, and opens the existing `/chat/{session_id}` experience. Inactive
Dots have no Chat action. Sessions remain private to their user and Dot, and
deactivation prevents new messages from starting. This first execution slice
loads Dot and user memory but exposes no tools until action policy is in place.

### Tool access and approvals walkthrough for review

The next personal-Dot settings surface extends each existing Dot card with an
`Access and approvals` disclosure. It uses the signed-in user's currently
available tools and connectors as the ceiling; it does not show tools the user
cannot use. The tool list comes from the runtime registry after user RBAC and
per-user tool settings are applied. MCP rows come from the existing connector
tool inventory, including each server's read-only annotation and the user's
current connector mode.

```text
Harald                                      Active
[Chat] [Memory] [Activity] [Access and approvals]

Access and approvals
Only tools already available to you can be enabled for Harald.

Tool label and description from catalog                  Dot rule             Identity
AIplane read-only tool from the user's catalog            Allow                AIplane
Read-only tool from connected Gira MCP                    Allow                My account
Write tool from connected Workspace MCP                  Ask before each call My account

Changes to these rules apply to new calls. A request already waiting in the
Inbox keeps the rule and arguments it was created with.
```

The list contains only tool IDs returned by the user's allowed-tool resolution
and connected MCP layers; display names and descriptions come from their
existing catalogs. `Allow` and `Ask before each call` map to the existing
`AgentSpec.main.tools` and `main.tool_resources[tool].permission` fields.
Omitting a tool from `main.tools` denies it to that Dot. The personal Dot's
effective set remains the intersection of this Dot selection and the user's
current grants and enabled settings. MCP `readOnlyHint` supplies the default
for read calls; a write or a tool without a read-only annotation defaults to
approval. The owner may change that default per tool within the user's grants.
The selected connector identity is resolved separately from this action rule;
a Dot-owned identity is offered only after that connector has a Dot-owned
credential.

An approval pauses the interactive Dot turn through the existing
`AskFirst`/`chat_turn_suspensions`/Inbox path. Approve-once resumes the pending
call; a saved `Allow` rule changes future calls. Background runs remain
fail-closed for approval-required actions until their workers can resume a
durably suspended run. The walkthrough does not add a parallel policy table or
approval mechanism.

## Delivery plan

### Slice 1: Dot ownership and active lifecycle

- Establish user- and organization-owned Dot records on top of existing agent
  definitions and principals.
- Let users create personal Dots when their configured group permission
  allows it. The permission mechanism remains an open implementation
  decision; the current lifecycle routes reuse `can_manage_agents`.
- Enforce the configurable active personal Dot cap (default 4) transactionally
  at creation/activation; deactivation frees capacity.
- Preserve current agent-manager workflows for visitor-facing and other
  principal-owned agents.
- Add the initial Dot list/create/deactivate API and state-based tests.

### Slice 2: Delegated personal execution

- Carry explicit `PersonalDot { user_id, dot_id }` attribution from the chat
  actor into `ToolContext`, while keeping the effective principal as the user.
  This establishes a scope for Dot-owned data; it does not yet grant tools or
  enforce a Dot action policy.
- Enforce user grants intersected with Dot policy before offering or executing
  a tool.
- Attribute every action to user and Dot in usage and audit.
- Keep sessions owner-scoped and private.

### Slice 3: MCP identity selection and action approval

- Reuse the MCP catalog, manager, OAuth refresh and secret sealing.
- Support selected user identity or Dot-owned identity for personal Dots.
- Support only Dot-/organization-owned identity for organization Dots.
- Add per-tool/action policy using the durable approval path.

### Slice 4: Persistent memory and workspace

- Use existing user memories for shared personal context.
- Add Dot-specific memory scope and bounded background curation.
- Reuse document/canvas and sandbox storage where they meet workspace
  isolation and persistence requirements; otherwise design one narrowly scoped
  Dot workspace store.

### Slice 5: Background work and isolated computer

- Extend scheduled and webhook execution to use Dot context and activity.
- Establish safe retries, cancellation, concurrency and usage bounds.
- Extend sandbox-runner for a persistent isolated browser/filesystem per Dot
  only after confirming the required lifecycle and deployment constraints.

### Slice 6: Organization management and operations

- Add organization ownership, manager access and organization/Dot connector
  provisioning.
- Add operator settings, limits, run visibility, retention and deployment
  documentation for the complete product.

Slices that change rights, personal-data access, credentials or external writes
need their policy and UI approved before those behaviors ship. Build one
vertical slice at a time; keep each feature's code, tests, translations,
documentation and generated artifacts together.

## Non-functional requirements

- No dependency on an external agent runtime, hosted thread store or hosted
  computer service.
- Existing AIplane model upstreams, Rust/Rama stack, SQLite migrations,
  JSON API, JSON-SSE, Fluent catalogs and daisyUI remain the platform
  conventions.
- Personal and organization data must be isolated in database queries and
  tool access, not merely filtered in the UI.
- Model/tool output remains untrusted. Existing injection defenses and
  outbound URL guard apply to background runs as well as interactive runs.
- Runs must respect configured model grants, cost limits, tool rules,
  suspension expiry, retention and audit requirements.
- The confirmed default of four limits active personal Dots, whether they are
  currently working or idle. The user has not requested a separate cap on
  simultaneous runs.

## Open decisions

1. Should personal-Dot creation use a dedicated gateway-group permission, the
   existing `can_manage_agents` permission, or a global operator switch?
2. Does an organization have its own active Dot cap, separate from personal
   limits?
3. Which exact action categories are always approval-required or prohibited?
   Who may approve organization actions?
4. For personal MCP identity, does "use my identity" reuse the existing
   `/tools/integrations` authorization, while "Dot identity" starts a separate
   OAuth flow? Which connector auth modes must work for Dot-owned identities?
5. How frequently may memory curation run, what evidence may it read, and are
   changes written automatically or reviewed by the user?
6. Does the Dot workspace need arbitrary structured collections, or are
   documents/files plus typed records sufficient? What size and retention
   bounds apply?
7. Must the Dot computer persist across process/container restarts, or only
   across agent runs? Which network destinations may it access?
8. What concurrent-run ceiling and resource budget should operators configure?

Deactivation is reversible and frees an active slot; this is the confirmed
product behavior and is implemented by the lifecycle API.

## Research references

- [OpenAI: Wir stellen dots vor](https://openai.com/de-DE/index/introducing-dots/)
- [OpenAI: How we build safety, security, and privacy into dots](https://openai.com/index/how-we-build-safety-security-and-privacy-into-dots/)
- [OpenAI Help Center: Getting started with your dot](https://help.openai.com/en/articles/20001530-getting-started-with-your-dot)
- [OpenAI Help Center: Dots privacy, security, and safety FAQs](https://help.openai.com/en/articles/20001529-dots-privacy-security-and-safety-faqs)
- [CopilotKit OpenDots repository README](https://github.com/CopilotKit/OpenDots/blob/main/README.md)
