# AIplane Dots: product and architecture specification

Status: implementation in progress; personal Dot ownership, active lifecycle,
the first web management page, manually managed Dot memory, recent activity,
owner-scoped memory/activity APIs, private Dot chat sessions, and personal
tool selection with durable approvals, per-action rules for `browse_page` and `browser_control`, Dot-bound schedules and webhooks, and same-owner directed delegation with durable child approvals are implemented.

The current implementation branch has the personal-Dot owner and active
state table, transactional active-slot enforcement, and session-authenticated
`GET`/`POST /api/v0/dots` plus `PUT /api/v0/dots/{id}/active` endpoints. The
active cap is an operator setting at `/admin/settings` → Access, defaulting to
four. The `/dots` page lists, creates, activates and deactivates personal Dots.
Lifecycle endpoints require the independent `can_manage_dots` group
permission; administrators hold it and other groups default to disabled. Each Dot card has a lazily loaded memory panel with separate
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
uses the tools selected in its Access and approvals panel, intersected with
the user's current grants, enabled settings and connected MCP tools. Existing
memory tools are selected at creation when granted and enabled; their writes
remain isolated to the Dot. Schedules and webhooks can run with the same personal Dot context and durable approvals.

| Endpoint | Behavior |
|---|---|
| `GET /api/v0/dots` | Lists only the signed-in user's personal Dots, including inactive ones, and returns the active limit. |
| `POST /api/v0/dots` | Creates a named personal Dot if the user has an active slot. |
| `PUT /api/v0/dots/{id}/active` | Activates or deactivates an owned Dot; activation rechecks capacity transactionally. |
| `GET /api/v0/dots/{id}/access` | Returns the owned Dot's tool selection and approval rules together with the user's current available catalog. |
| `PUT /api/v0/dots/{id}/access` | Replaces the owned Dot's selected tools and their approval rules, retaining other agent spec fields. |
| `GET /api/v0/dots/{id}/communication` | Returns actual same-owner peers and separate directed outgoing/incoming consent. |
| `PUT /api/v0/dots/{id}/communication` | Saves one directed consent under current management permission and generation checks. |
| `GET /api/v0/dots/{id}/memories` | Lists the owned Dot's memories. |
| `POST /api/v0/dots/{id}/memories` | Creates or reclassifies a Dot memory (`preference`, `project`, `fact`). |
| `PUT /api/v0/dots/{id}/memories/{memory_id}` | Updates an owned Dot memory. |
| `DELETE /api/v0/dots/{id}/memories/{memory_id}` | Deletes an owned Dot memory. |
| `GET /api/v0/dots/{id}/activity` | Lists the owned Dot's audited activity, newest first; accepts `cursor` and `limit`. |
| `POST /api/v0/dots/{id}/sessions` | Creates a private chat session owned by the user and associated with the active Dot. |

Lifecycle routes require `can_manage_dots`. Administrators configure it in the
existing group form; agent management does not imply Dot management. Revoking
it blocks list, creation and active-state changes after RBAC reload. It does
not deactivate existing Dots, revoke tool grants or stop existing runs;
owner-scoped access and execution retain their existing checks. Creation and active-state
changes are recorded through the existing agent activity chain. The Dot
principal starts without independent grants; personal execution uses the
owner's effective authority. Lowering the configured active cap does not deactivate existing
Dots; creation and reactivation remain blocked until the owner is below the
new cap. Dot memories are separate from `user_memories`, reuse its kinds and
duplicate handling, and can be managed under `/api/v0/dots/{id}/memories`.
A Dot chat session reads bounded Dot memory and the user's existing profile
context. The memory tools `remember`, `recall`, `update_memory` and `forget`
follow the selected Dot rules and the user's memory grant and setting. Writes go to that
Dot's own store; recall returns both Dot and user memories with explicit
scopes, and user-wide memory is read-only from a Dot chat. Selected core and
MCP tools act under the owner account and existing connector mode. Deactivating
a Dot preserves its existing sessions but blocks new sessions and new turns;
ordinary pending Dot turns remain paused while the Dot is inactive. Affected
delegation trees are cancelled when a participating Dot is deactivated. The `/dots`
page displays active usage against the operator limit, creates a personal Dot
with a friendly name, starts a private chat, and lets the owner deactivate or
reactivate it. A deactivated Dot stays in the list and can be reactivated when
a slot is available. The Access and approvals disclosure offers selected tools
with `Allow` or `Ask before each call`; unselected tools are denied. Connector
identity labels identify the selected user, Dot or shared connection. Supported
personal OAuth connectors may use a separately authorized Dot connection;
connecting and selecting it are separate operations. Organisation-owned Dots
remain unimplemented. An owned local credential remains visible for explicit
disconnect after catalogue access is lost, including credentials never selected
for execution. Restricted catalogue and user-account details stay hidden.
Active
Dot cards link to the existing schedule and webhook creation forms with that
Dot selected.

Access changes reuse the agent's draft spec, revisions and audited save path.
The API accepts `{"tools":[{"id":"browse_page","permission":"always_ask","actions":[{"id":"read","rule":"allow"},{"id":"fill","rule":"deny"}]}]}`;
an empty list denies every tool. Duplicate IDs, unavailable tools and unknown
permissions are rejected. Only the owner can read or update these settings,
including while the Dot is inactive. Tool labels and descriptions use the
existing catalog and live tool schemas; actual IDs distinguish actions within
a grouped capability. A read-only declaration defaults to Allow; writes and
unknown effects default to Ask before each call. Explicit rules may narrow or
permit an action within the owner's existing authority. A connector's own
Ask mode continues to require approval even when the Dot rule is Allow.

Tools with a declared action catalogue expose an **Actions** disclosure. Each
actual action can inherit the tool rule or use Allow, Ask or Deny. The implementation supports `browse_page` and batched `browser_control`; connector tools without that contract
retain their existing tool rules. Action IDs and descriptions come from the
tool implementation. Unavailable saved actions remain visible and must be
removed before saving. Current overrides and grants are checked before any
execution, including after approval; action Allow cannot bypass connector Ask.
The saved pending call carries ordered action IDs, and incompatible changes
to its classification refuse execution. See [action rules](dots-action-rules.md)
for the runtime and wire contract.

AskFirst persists a suspended call in the existing chat approval system. The
owner can approve once or deny in chat or Inbox. Approval refers to the original
arguments; resumption reloads Dot selection, current user grants, enabled
settings and connected tools. Revoked selections and disabled tools cannot
execute on an old approval. Dot deactivation leaves ordinary pending calls
paused and cancels affected delegation trees.

## Personal Dot communication consent

The **Communication** disclosure configures separate **Can ask** and **Accepts
requests from** decisions for actual owned Dots. Both decisions are required
for a directed relationship; reverse communication needs its own choices.
Inactive peers remain visible. New enabling requires both Dots active, while
revocation remains possible. Each row saves separately and a stale edit must
be reloaded explicitly. Mutations require current `can_manage_dots` permission.
The **Both consents enabled** indicator describes stored consent. Personal chats with selected and authorized `forward_request` can send an
explicit bounded task to a consenting receiver. The receiver uses its own
selected tools, rules and memories under current owner permissions. Child
approvals propagate to the root conversation and Inbox; receiver histories
are private and read-only. Revoking consent or deactivating a participating
Dot cancels affected delegation trees. See [Dot communication](dots-communication.md).

## Implemented personal Dot background execution

The existing schedule and webhook forms expose **Run as**, using the owner's
actual Dots and the ordinary assistant option. Creating or changing a binding
requires an active owned Dot. An unchanged inactive or unavailable binding
remains visible and may be retained while editing unrelated configuration;
it cannot execute until the binding is available and active. There is no
independent Dot model selector: the existing model catalogue, cron/timezone,
tools switch, synchronous mode and reuse settings remain the controls.

Conversation reuse requires a private user-owned session with exactly the
same Dot binding, or an ordinary private session for an ordinary automation.
The linked-chat picker uses the existing chat listing with `dot_id` filtering:
an absent filter lists all, an empty filter selects ordinary sessions, and a
concrete ID selects that user's Dot sessions. Changing **Run as** clears the
form's linked conversation; server validation repeats the ownership and
binding checks. Webhook reruns replay an actual stored payload into a fresh
conversation carrying the webhook's current binding.

`headless::open_session` uses session-core's atomic idle turn-pair creation:
it inserts the user prompt and assistant turn together only if the conversation
has no running or suspended assistant turn. Background execution registers
with the same worker registry as chat, so streaming and stopping use the shared
session mechanisms. The substrate remains unaware of Dot ownership.

Each run records the exact assistant turn ID, original tools switch and history
window before model execution. Resume reloads that durable run context and
checks its owner, private session and unchanged Dot binding. The tools ceiling
is the original switch intersected with the current automation switch, owner
grants, enabled settings and Dot rules. Enabling tools later cannot grant them
to a turn that started without them. The recorded history window is restored
on resume; later settings cannot broaden that turn's history. Usage retains
its scheduled/webhook source and existing owner billing and limits. A separate
per-Dot usage dashboard or billing budget is not implemented.

Approval-required calls enter `waiting`, appear in Inbox and the linked chat,
and release the execution worker. Approval applies only to the stored call
arguments, with current access checked before execution. Resumed completion,
denial or cancellation updates the exact run; an older continuation cannot
overwrite the summary of a newer run. Synchronous webhook fires and reruns
return HTTP 202 with `status: waiting` and the session ID when a decision is
needed; waiting is distinct from a failed run.

Pausing a schedule or webhook prevents future triggers and permits resolution
of its existing pending turn. Deactivating the Dot preserves conversations
and keeps ordinary pending Dot calls paused; affected delegation trees are
cancelled. Deletion of an automation returns a
conflict while its run is executing or awaiting a decision: disable future
triggers, then finish or stop the turn before deleting. The existing Dot UI
provides deactivation, not a Dot-delete action.

Persistent Dot computers are specified separately in
[dots-workspace.md](dots-workspace.md) and are not implemented. Automatic
memory curation and organization Dots remain
planned. The user workflow is in
[Schedules, webhooks and inbox](guide/automation-and-inbox.md).

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

- Dots must be able to communicate with one another under explicit permissions
  in both directions. The first planned slice permits bounded internal
  delegation between personal Dots with the same owner; receivers retain their
  own tools and approval rules, and only explicit task data is shared. This is
  not executable yet; directed consent configuration is implemented and the
  reusable dispatch and privacy design is in
  [Dot communication](dots-communication.md).

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
| Durable pause and approval | `chat_turn_suspensions`, `agents::approval`, Inbox | Interactive chat and user-owned scheduled/webhook turns can persist a pause and resume through the user's Inbox. Dot execution must retain its attribution and re-evaluate current owner grants and Dot rules when a paused turn resumes. |
| Schedules | `scheduled_actions` and scheduled worker | Personal Dot binding is implemented; organization-owned execution remains planned |
| Event triggers | Webhooks and headless execution | Personal Dot binding and durable assistant-turn correlation are implemented; organization ownership remains planned |
| MCP catalogue and execution | connector catalog, `McpConnectionManager`, `UserMcpLayer` | Personal delegated and Dot-owned credentials; organisation/Dot OAuth identity |
| Per-user memory | `user_memories`, memory tools, `personal_dot_memories` | Runtime reads/writes and background curation for isolated Dot memory |
| Agent conversation state | `agent_state` | Session-scoped today; Dot workspace must persist independently across sessions |
| Browser and code execution | sandbox-runner, sandbox browser tools, browser-control | Persistent isolated computer/profile/storage per Dot, suitable for background runs |
| Operator settings | declarative settings API and SPA | Dot creation permission and active-Dot limit (default 4); define organization scope |
| Usage controls | user/agent spend and usage limits | Attribute background usage to its owner and Dot; no separate parallel-run cap is requested |

### Shared mechanisms: personal Dot access and approvals

The current code establishes these reuse points and the remaining runtime seam:

| Need | Existing mechanism | Dot-specific work |
|---|---|---|
| Resolve the signed-in user's current tool ceiling | `AppState::allowed_tools_for_session`, `UserMcpLayer`, and `CompositeToolSource` | Resolve the ceiling on each Dot turn and compose only the connectors selected for that Dot. Do not treat a tool name from the model as a grant. |
| Store a Dot's tool allowlist and approval settings | The Dot already has an `agents.draft_spec`; `AgentSpec.main.tools` and `main.tool_resources` represent tool selection and per-tool permission | Add an owner-scoped update/read path that validates against the user's current grants. The ordinary agent spec validator assumes grants belong to the agent principal, so it cannot be called with the Dot's empty principal grants unchanged. Do not add a parallel action-policy table. |
| Enforce tool rules before execution | `RunToolSource`, `GrantedToolSource`, `Permissions` and `AskFirst` | `ChatPersona::with_tool_access` narrows the dynamic user/MCP source. `LivePersonalDotTool` reloads active ownership, current user roles, selected rules, enabled settings and the actual connected tool before each invocation, then applies the existing approval wrapper. Current access also intersects the turn's initial grant ceiling. |
| Pause and resume a decision | `chat_turn_suspensions`, `AskFirst`, the user Inbox, and `resume_turn` | The chat resume path reloads the session's `personal_dot_sessions` link before constructing the driver. Keep that resolution and re-check the live owner grant and Dot rule when resuming. |
| Attribute tool activity | `agent_audit` and the personal-Dot attribution on `ToolContext` | Records the acting user, Dot, tool, decision and outcome. Personal MCP admission records the currently selected typed connection and generations as the identity of the attempted call; outcomes remain correlated by the existing call ID. |

`Tool::changes_state()` is the existing default for agent approval and an MCP
tool reports a state change from its server's destructive and read-only
annotations. A personal Dot needs a stricter default: missing or incomplete
read-only metadata must not silently turn an external action into an
unreviewed write. This stricter resolution belongs in the Dot policy seam; it
must not change permission defaults for ordinary chat or existing agents.

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

Schedules and webhooks retain their user ownership and optionally bind to a
personal Dot through `dot_id`. Their headless workers open private user-owned
sessions carrying the Dot binding and resolve the same personal Dot persona
as chat. `tools_enabled` is an additional ceiling, not approval for external
writes. Webhook payloads retain their untrusted-input framing. Organization
background ownership is not implemented.

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

Interactive chat and the existing user-owned scheduler/webhook workers can
persist a suspension in `chat_turn_suspensions`, release the worker and resume
the same tool call through the user's Inbox. Dot execution can reuse this
continuation when it keeps the Dot context attached to the user-owned session
and verifies permissions again on resume. A tool that requires a decision
must still refuse to run on a path where nobody can resume it; enabling tools
on a schedule or webhook is not itself approval. Organization-owned runs need
an explicitly authorized approval owner and continuation path.

Exact built-in action categories, whether creating a draft is itself a write,
and which organization members may approve are open design decisions. Until
those are decided, the implementation must not silently enable external
write actions.

## Background work and memory curation

AIplane schedules and webhooks use the existing workers with an optional
personal Dot binding. MCP calls may be used as event-driven work inputs where the connected
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
loads Dot and user memory and applies the selected tool and approval rules.

### Approved tool access and approvals walkthrough

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

Changes to these rules apply to new calls. Approval applies only to the pending
arguments. Current permissions and Dot availability are checked again on resume.
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
call; a saved `Allow` rule changes future calls. Scheduled and webhook runs
persist approval-required calls and resume through the same Inbox path. The walkthrough does not add a parallel policy table or
approval mechanism. On resume, current ownership, active state and tool access
are checked again even when the call was approved once.

## Delivery plan

### Slice 1: Dot ownership and active lifecycle

- Establish user- and organization-owned Dot records on top of existing agent
  definitions and principals.
- Let users create personal Dots when their configured group permission
  allows it through the independent `can_manage_dots` group flag.
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

1. Does an organization have its own active Dot cap, separate from personal
   limits?
2. Which exact action categories are always approval-required or prohibited?
   Who may approve organization actions?
3. For personal MCP identity, does "use my identity" reuse the existing
   `/tools/integrations` authorization, while "Dot identity" starts a separate
   OAuth flow? Which connector auth modes must work for Dot-owned identities?
4. How frequently may memory curation run, what evidence may it read, and are
   changes written automatically or reviewed by the user?
5. Does the Dot workspace need arbitrary structured collections, or are
   documents/files plus typed records sufficient? What size and retention
   bounds apply?
6. Must the Dot computer persist across process/container restarts, or only
   across agent runs? Which network destinations may it access?
7. What concurrent-run ceiling and resource budget should operators configure?

Deactivation is reversible and frees an active slot; this is the confirmed
product behavior and is implemented by the lifecycle API.

## Research references

- [Grok Bot feature comparison and additional product gaps](dots-grok-comparison.md)
- [xAI: Introducing Grok Bot](https://x.ai/news/introducing-grok-bot)
- [xAI: Grok Bot product page and FAQ](https://x.ai/bot)
- [OpenAI: Wir stellen dots vor](https://openai.com/de-DE/index/introducing-dots/)
- [OpenAI: How we build safety, security, and privacy into dots](https://openai.com/index/how-we-build-safety-security-and-privacy-into-dots/)
- [OpenAI Help Center: Getting started with your dot](https://help.openai.com/en/articles/20001530-getting-started-with-your-dot)
- [OpenAI Help Center: Dots privacy, security, and safety FAQs](https://help.openai.com/en/articles/20001529-dots-privacy-security-and-safety-faqs)
- [CopilotKit OpenDots repository README](https://github.com/CopilotKit/OpenDots/blob/main/README.md)
