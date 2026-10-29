# AIplane agents: product and architecture specification

Status: implementation in progress; personal agent ownership, active lifecycle,
the first web management page, manually managed agent memory, recent activity,
owner-scoped memory/activity APIs, private agent chat sessions, and personal
tool selection with durable approvals, per-action rules for `browse_page` and `browser_control`, agent-bound schedules and webhooks, and same-owner directed delegation with durable child approvals are implemented. Organization ownership, group-derived membership, independent active limits and configuration through the existing agent builder are implemented; current member writers can authorize and explicitly select each organization agent’s own OAuth credential. Active organization agents support private saved-draft and published conversations for current member writers, including durable approvals, selected own credentials, isolated agent notes and shared organization knowledge. Published private conversations retain their original version and use current member Write access for history and Inbox decisions. Organization schedules and asynchronous webhooks run pinned published versions in fresh private principal conversations. Personal agents can delegate bounded tasks to consented organization agents. Organization conversation reuse, outgoing organization delegation and public agent channels remain unavailable.

The current implementation branch has the personal-agent owner and active
state table, transactional active-slot enforcement, and session-authenticated
`POST /api/v0/agents` (with `kind: "personal"`) plus
`PUT /api/v0/agents/{id}/active` endpoints. The
active cap is an operator setting at `/admin/settings` → Access, defaulting to
four. In the product a personal agent is a **personal agent**: the `/agents` list
shows it under *My agents* and its editor opens at `/agents/{id}`.
Lifecycle endpoints require the independent `can_manage_personal_agents` group
permission; administrators hold it and other groups default to disabled. Each
card in the agent list shows its name, the *Personal* badge, its active state
and optional purpose description. Its editor has the tabs **Task** (see
[personal agent profile](#personal-agent-profile)), **Tools** (access and
approvals), **Memory**, **Activity**, **Collaboration** (communication),
**Files** (the agent's file manager) and **Automation** (the existing agent-scoped
schedule and webhook pages). Tabs keep their loaded state while switching so
unsaved access edits are preserved.
The short description appears on the card and to other agents; it is not sent to
the model as an instruction. Conversational setup saves separate work
instructions that the personal agent uses in its runs. The manual creation
dialog leaves those instructions unset; give that agent its task in chat. See
[the setup walkthrough](guide/automation-and-inbox.md#set-up-a-personal-agent-in-conversation).
The memory tab has
separate preference, project and fact entries. Owners can add, edit and delete
those notes; they remain separate from user-wide memory. The activity tab
shows the agent's recent audited events and can page through older ones.
Starting an agent chat adds a session-start event without copying conversation
text. Personal agent tool decisions and outcomes appear in the same feed with
the acting user and agent attribution; their activity detail contains the tool,
decision and status, but not arguments or tool results.
Agent memory has a separate store and owner-scoped API, but automatic memory
curation is not implemented. A personal agent can start a private chat that
reads bounded agent memory and the user's existing profile context. Its chat
uses the tools selected in its Access and approvals panel, intersected with
the user's current grants, enabled settings and connected MCP tools. A personal
agent created by hand starts with the [starter tools](#starter-tools) its
owner holds; memory writes remain isolated to the agent. Schedules and webhooks
can run with the same personal agent context and durable approvals.

| Endpoint | Behavior |
|---|---|
| `GET /api/v0/agents` | Lists the caller's workspace; a personal agent has `kind: "personal"` and `ownership` with its name and active state, inactive ones included. |
| `POST /api/v0/agents` | With `kind: "personal"`, creates a named personal agent if the user has an active slot. |
| `PUT /api/v0/agents/{id}/active` | Activates or deactivates an owned agent, or another user's for an admin; activation rechecks the owner's capacity transactionally. |
| `GET /api/v0/agents/{id}/profile` | Returns the agent name, the display name, description, task, tone and topics with the owner and a `revision`. |
| `PUT /api/v0/agents/{id}/profile` | Replaces the display name, description, task, tone and topics in one audited save; a stale `revision` answers `409`. |
| `GET /api/v0/agents/{id}/access` | Returns the agent's tool selection and approval rules together with its owner's current available catalog. |
| `PUT /api/v0/agents/{id}/access` | Replaces the owned agent's selected tools and their approval rules, retaining other agent spec fields. |
| `GET /api/v0/agents/{id}/communication` | Returns actual same-owner peers and separate directed outgoing/incoming consent. |
| `PUT /api/v0/agents/{id}/communication` | Saves one directed consent under current management permission and generation checks. |
| `GET /api/v0/agents/{id}/memories` | Lists at most 500 notes with actual scopes/names/current writable flags; organization access requires current membership and relevant Read/Write share. |
| `POST /api/v0/agents/{id}/memories` | Creates or reclassifies a memory (`preference`, `project`, `fact`); requires explicit `scope` and at most 2,000 UTF-8 bytes. |
| `PUT /api/v0/agents/{id}/memories/{memory_id}` | Updates a note under explicit `scope` and the same byte bound and current write rights. |
| `DELETE /api/v0/agents/{id}/memories/{memory_id}` | Deletes an authorized note under explicit `?scope=agent` or `?scope=organization`. |
| `GET /api/v0/agents/{id}/activity` | Lists the owned agent's audited activity, newest first; accepts `cursor` and `limit`. |
| `POST /api/v0/agents/{id}/sessions` | Creates a private chat session owned by the user and associated with the active agent. |

Lifecycle routes require `can_manage_personal_agents`. Administrators configure it in the
existing group form; agent management does not imply agent management. Revoking
it blocks list, creation and active-state changes after RBAC reload. It does
not deactivate existing agents, revoke tool grants or stop existing runs;
owner-scoped access and execution retain their existing checks. Creation and active-state
changes are recorded through the existing agent activity chain. The agent
principal starts without independent grants; personal execution uses the
owner's effective authority. Lowering the configured active cap does not deactivate existing
Agents; creation and reactivation remain blocked until the owner is below the
new cap. Agent memories are separate from `user_memories`, reuse its kinds and
duplicate handling, and can be managed under `/api/v0/agents/{id}/memories`.
An agent chat session reads bounded agent memory and the user's existing profile
context. The memory tools `remember`, `recall`, `update_memory` and `forget`
follow the selected agent rules and the user's memory grant and setting. Writes go to that
Agent's own store; recall returns both agent and user memories with explicit
scopes, and user-wide memory is read-only from an agent chat. Selected core and
MCP tools act under the owner account and existing connector mode. Deactivating
an agent preserves its existing sessions but blocks new sessions and new turns;
ordinary pending agent turns remain paused while the agent is inactive. Affected
delegation trees are cancelled when a participating agent is deactivated. The `/agents`
page displays active usage against the operator limit, creates a personal agent
with a friendly name, starts a private chat, and lets the owner deactivate or
reactivate it. A deactivated agent stays in the list and can be reactivated when
a slot is available. The Access tab offers selected tools
with `Allow` or `Ask before each call`; unselected tools are denied. Connector
identity labels identify the selected user, agent or shared connection. Supported
personal OAuth connectors may use a separately authorized agent connection;
connecting and selecting it are separate operations. Organization agents have
explicit organization ownership, own-account management and private draft or published
test chat for current member writers. Their Setup memory panel exposes isolated
Agent notes and shared organization knowledge. Member writers change own notes;
shared-note changes also require current gateway administration. Runtime memory
reads both scopes under its Recall grant; writes remain agent-specific and no
employee profile is read. Organization schedules and webhooks use the selected published
version and independent principal. They can receive consented personal agent
requests; outgoing organization delegation and public agent channels remain
unavailable. An owned local credential remains visible
for explicit
disconnect after catalogue access is lost, including credentials never selected
for execution. Restricted catalogue and user-account details stay hidden.
Active
Agent cards link to the existing schedule and webhook creation forms with that
Agent selected.

Access changes reuse the agent's draft spec, revisions and audited save path.
The API accepts `{"tools":[{"id":"browse_page","permission":"always_ask","actions":[{"id":"read","rule":"allow"},{"id":"fill","rule":"deny"}]}]}`;
an empty list denies every tool. Duplicate IDs, unavailable tools and unknown
permissions are rejected. Only the owner can read or update these settings,
including while the agent is inactive. Tool labels and descriptions use the
existing catalog and live tool schemas; actual IDs distinguish actions within
a grouped capability. A read-only declaration defaults to Allow; writes and
unknown effects default to Ask before each call. Explicit rules may narrow or
permit an action within the owner's existing authority. A connector's own
Ask mode continues to require approval even when the agent rule is Allow.

Tools with a declared action catalogue expose an **Actions** disclosure. Each
actual action can inherit the tool rule or use Allow, Ask or Deny. The implementation supports `browse_page` and batched `browser_control`; connector tools without that contract
retain their existing tool rules. Action IDs and descriptions come from the
tool implementation. Unavailable saved actions remain visible and must be
removed before saving. Current overrides and grants are checked before any
execution, including after approval; action Allow cannot bypass connector Ask.
The saved pending call carries ordered action IDs, and incompatible changes
to its classification refuse execution. See [action rules](agents-action-rules.md)
for the runtime and wire contract.

AskFirst persists a suspended call in the existing chat approval system. The
owner can approve once or deny in chat or Inbox. Approval refers to the original
arguments; resumption reloads agent selection, current user grants, enabled
settings and connected tools. Revoked selections and disabled tools cannot
execute on an old approval. Agent deactivation leaves ordinary pending calls
paused and cancels affected delegation trees.

## Personal agent profile

The **Task** tab of a personal agent edits what it is called and told to do:
its display name (the principal's; the agent name it was created with stays
unchanged), its card description,
and three fields of its draft spec — `main.instructions.orchestration` (task),
`main.instructions.response` (tone) and `scope.topics`. The personal agent
persona briefs every chat, schedule and webhook run from those fields
(`render_owner_brief`), so a save takes effect from the next turn; there is no
publish step for a personal agent. Blank fields are removed from the spec and
every other spec field is kept. Limits: 80 characters for the display name, 500 for the
description, 20,000 each for task and tone, 50 topics of 200 characters.

### Administrator changes

The settings routes — profile, access and active state — also answer an
administrator for another user's personal agent. They resolve the owner and
check every setting against the owner's grants and connector catalogue, never
the administrator's; the save is audited with the administrator as actor. The
editor shows such an administrator only **Task** and **Tools** with a notice
naming the owner. Memory, activity, communication, connections, files,
sessions and runs stay owner-only and answer `404` to everyone else. A personal
agent cannot be shared ([`agents.md`](agents.md#shares)).

## Starter tools

`POST /api/v0/agents` selects, among the tools its owner holds
(`allowed_tools_for_user`), the memory tools and `STARTER_TOOLS` in
`json_owned_agents.rs` — `get_current_timestamp`, `search_web`, `fetch_url`,
`rag_search`, `schedule_action` and `forward_request` (*Ask another agent*) —
so a new personal agent can research, schedule and delegate before anyone
opens its **Tools** tab. Each rule comes from the same `default_permission`
the conversational setup uses: the memory tools, read-only tools and
`forward_request` are allowed, everything else asks before each call.
*Chosen:* `forward_request` reaches no other agent until both consents of the
pair are set ([communication](agents-communication.md)) — that is the owner's
permission — and the receiver keeps its own rules, so asking again on every
delegation would only get in the way. The owner can set any of these to Ask,
or allow scheduling, on the **Tools** tab. A tool the owner
does not hold is not selected, and every selection is checked against the
owner's current grants on each call. The conversational setup keeps its
reviewed proposal and selects only what the proposal shows.

The model sees each personal agent, and each agent it may ask, by its display
name, the name people use for it.

## Personal agent files

Choose **Workspace** on a personal agent card to open its files. The page uses
that agent’s actual name and storage state. Open folders, download files, upload
files of at most 50 MiB, create folders, or delete files and empty folders after
confirmation. Missing file metadata is shown as unavailable. Directory results
are paginated; reload after changes from a running agent. Files are downloaded,
not executed or previewed in the web page.

The owner can inspect an inactive agent. Upload, folder creation and deletion are
blocked while any of the agent’s computer containers are active or waiting for
confirmed teardown. Owner rights are checked again after storage requests.
Files remain across turns and container expiry. In verified personal agent
`run_in_sandbox` and `browse_page` calls, `/workspace` is the shared persistent
file directory; `/work` remains scratch and attachment output. Copy a selected
output into `/work` to deliver it in the conversation.

Persistent storage is available only with a configured persistent runner volume
and a successful local rootful Linux/gVisor mount proof at runner startup. An
unverified runner shows unavailability; it does not substitute scratch storage.
The workspace API supports personal ownership. Browser logins persist in the
Agent's server-side browser profile; a self-service browser-profile reset is not
implemented. See the complete
[workspace design](agents-workspace.md) and [runner requirements](sandbox.md).

Deactivation preserves files. Agent and account deletion create durable cleanup
work. The runner seals the deleted agent, confirms container removal, then removes
its files in bounded passes. Failed cleanup retries after restart. A private
minimal owner/agent tombstone remains to reject delayed requests; it contains no
workspace contents and is not exposed to users or models.

## Personal agent communication consent

The **Communication** disclosure configures separate **Can ask** and **Accepts
requests from** decisions for actual owned agents. Both decisions are required
for a directed relationship; reverse communication needs its own choices.
Inactive peers remain visible. New enabling requires both agents active, while
revocation remains possible. Each row saves separately and a stale edit must
be reloaded explicitly. Mutations require current `can_manage_personal_agents` permission.
The **Both consents enabled** indicator describes stored consent. Personal chats with selected and authorized `forward_request` can send an
explicit bounded task to a consenting receiver. The receiver uses its own
selected tools, rules and memories under current owner permissions. Child
approvals propagate to the root conversation and Inbox; receiver histories
are private and read-only. Revoking consent or deactivating a participating
Agent cancels affected delegation trees. See [Agent communication](agents-communication.md).

## Implemented personal agent background execution

The existing schedule and webhook forms expose **Run as**, using the owner's
actual agents and the ordinary assistant option. Creating or changing a binding
requires an active owned agent. An unchanged inactive or unavailable binding
remains visible and may be retained while editing unrelated configuration;
it cannot execute until the binding is available and active. There is no
independent agent model selector: the existing model catalogue, cron/timezone,
tools switch, synchronous mode and reuse settings remain the controls.

Conversation reuse requires a private user-owned session with exactly the
same agent binding, or an ordinary private session for an ordinary automation.
The linked-chat picker uses the existing chat listing with `agent_id` filtering:
an absent filter lists all, an empty filter selects ordinary sessions, and a
concrete ID selects that user's agent sessions. Changing **Run as** clears the
form's linked conversation; server validation repeats the ownership and
binding checks. Webhook reruns replay an actual stored payload into a fresh
conversation carrying the webhook's current binding.

`headless::open_session` uses session-core's atomic idle turn-pair creation:
it inserts the user prompt and assistant turn together only if the conversation
has no running or suspended assistant turn. Background execution registers
with the same worker registry as chat, so streaming and stopping use the shared
session mechanisms. The substrate remains unaware of agent ownership.

Each run records the exact assistant turn ID, original tools switch and history
window before model execution. Resume reloads that durable run context and
checks its owner, private session and unchanged agent binding. The tools ceiling
is the original switch intersected with the current automation switch, owner
grants, enabled settings and agent rules. Enabling tools later cannot grant them
to a turn that started without them. The recorded history window is restored
on resume; later settings cannot broaden that turn's history. Usage retains
its scheduled/webhook source and existing owner billing and limits. A separate
per-agent usage dashboard or billing budget is not implemented.

Approval-required calls enter `waiting`, appear in Inbox and the linked chat,
and release the execution worker. Approval applies only to the stored call
arguments, with current access checked before execution. Resumed completion,
denial or cancellation updates the exact run; an older continuation cannot
overwrite the summary of a newer run. Synchronous webhook fires and reruns
return HTTP 202 with `status: waiting` and the session ID when a decision is
needed; waiting is distinct from a failed run.

Pausing a schedule or webhook prevents future triggers and permits resolution
of its existing pending turn. Deactivating the agent preserves conversations
and keeps ordinary pending agent calls paused; affected delegation trees are
cancelled. Deletion of an automation returns a
conflict while its run is executing or awaiting a decision: disable future
triggers, then finish or stop the turn before deleting. The existing agent UI
provides deactivation, not an agent-delete action.

Personal agent files and persistent browser profiles are implemented for
supported runner deployments; owner browser handover is described in
[agents-browser-handover.md](agents-browser-handover.md). Automatic memory
curation remains planned. Organization conversation reuse remains unavailable.
The user workflow is in
[Schedules, webhooks and inbox](guide/automation-and-inbox.md).

## Organization schedules and private run history

Current organization member writers open **Scheduled actions** from the agent's
existing agent editor. The same list and create/edit forms use the actual
`principal_id` scope. Back, Cancel and successful saves retain that scope;
personal schedules retain their existing user scope and optional agent binding.

An organization schedule explicitly selects an available positive published
version and timezone. The form displays the selected version's authored model;
an omitted model is shown as missing. It offers no employee model override,
user identity or linked personal conversation. Each fire opens a fresh private
conversation owned by the agent's principal. The selected version remains pinned
after a newer publish. Current principal model/tool grants and explicitly
selected own MCP credentials remain execution ceilings; no employee profile,
credential or model grant is substituted.

Schedule edits and toggles compare the returned configuration generation.
Authored execution edits invalidate older runs for further execution. Disable
prevents future fires while leaving an admitted pending run answerable. Run
history retains its original version and generation. Structural cancellation
and reconciliation preserve the original owner, private session, assistant turn
and run/session version; changed configuration or deactivation does not prevent
an authorized writer from stopping that exact turn. Reconciliation updates the
exact run and cannot replace a newer or different-generation schedule summary.

**Open chat** opens the existing editor at
`/agents/{principal_id}?tab=try&test_session={session_id}`. Test chat reads the
actual private transcript over its existing events endpoint. Its source and
version remain unknown until the stopped-turn debug response supplies them;
source selection and submission remain disabled during that load. No Draft
label is inferred for a scheduled conversation. **Stop** cancels the actual
running or suspended turn through
`POST /api/v0/agents/{id}/test/{session_id}/cancel` with its `turn_id`.
A cancelled turn is recorded as a failed run with its cancellation reason.
**New conversation** clears the history query without cancelling existing work.
Navigation invalidates outstanding component operations so an older response
cannot reopen a previous session or overwrite the current URL.

Organization schedule conversation reuse, agent delegation and public agent
channels are unavailable. Organization webhooks use the same private history
and exact-turn Stop; see [their admission contract](agents-webhooks.md). Personal
Agent computers and browser handover are implemented for supported runner
deployments; automatic memory curation remains planned. See [organization agents](agents-organizations.md) and
[Schedules, webhooks and inbox](guide/automation-and-inbox.md).

## Product goal

AIplane agents are named, persistent AI coworkers that a person or an
organisation can reach through AIplane's web interface. An agent keeps useful
context across runs, can work from chat, schedules and incoming events, and
uses only the identities, connectors and actions its owner has authorised.
The implementation uses AIplane's own runtime and configured model
upstreams. It does not install or depend on OpenDots, CopilotKit Intelligence,
AG-UI, OpenBot or OpenAI Dots.

The product adopts the user-facing capabilities described by OpenAI Dots and
OpenDots, not their code or hosted services. OpenAI describes scheduled work,
read-only background research, long-term preferences and action review. The
OpenDots repository describes a persistent agent workspace, per-agent
connections and approvals, and explicitly notes that its schedules are not a
complete goal or event-trigger system. These references inform the product
requirements; they do not determine AIplane's implementation.

## Confirmed requirements

### Agent ownership and identity

- A personal agent belongs to one user, has its own display name, and runs with
  that user's effective permissions.
- A personal agent may use a connector with the user's connected identity or a
  connector identity attached specifically to that agent. The user chooses per
  connector.
- An organisation agent belongs to an organisation, has its own system identity,
  and must use an organisation/agent-owned connector identity. It must not
  silently inherit an employee's identity.
- The agent name is the named agent identity shown to people. For a personal agent,
  the acting user remains the security principal for permission checks; the
  activity record also identifies the agent.
- Users may create agents when the operator enables that capability. The
  operator controls the maximum number of active personal agents per user.
- The initial default active-agent limit is four. Deactivating an agent frees an
  active slot; the requirement limits active agents, not the historical number
  of disabled agents.
- Deleting a user removes that user's personal agent principals, definitions,
  memories, grants and tokens through the existing principal cascade.

### Work and channels

- Agents must be able to communicate with one another under explicit permissions
  in both directions. The first planned slice permits bounded internal
  delegation between personal agents with the same owner; receivers retain their
  own tools and approval rules, and only explicit task data is shared. This is
  not executable yet; directed consent configuration is implemented and the
  reusable dispatch and privacy design is in
  [Agent communication](agents-communication.md).

- The initial user interface is AIplane's web application only. Slack is out
  of scope. Voice and other channels are later work unless separately
  approved.
- Agents are reachable when idle and can continue work through scheduled or
  event-triggered runs. "Always on" does not require a model process to run
  continuously.
- Events should reuse AIplane's existing webhook and tool/MCP mechanisms. A
  ticket-monitoring example is handled through the configured ticket-system
  MCP; it does not justify a Gira/Jira-specific integration.
- Agents use AIplane's configured model catalogue and existing upstream routing.
- Existing sandbox and browser capabilities should be reused or extended to
  give an agent a suitably isolated computer. The browser extension that acts in
  a user's live browser is not sufficient for unattended work.

### Permissions and actions

- A personal agent's effective permissions are the intersection of the user's
  effective permissions and the agent's own allowed tools/actions. The agent may
  narrow user rights but never expand them.
- An organisation agent uses its own principal and explicit grants.
- Owners choose connectors and configure rules by tool and action.
- Actions outside read-only research and chat require an explicit policy.
  External writes and critical actions must be reviewable and can require a
  human approval before execution.
- Approval checks are enforced in the gateway/tool path, not only in prompts
  or the browser.
- The activity record must show the acting user (for personal agents), the agent,
  the selected connector identity, action, decision and outcome.

### Memory and agent workspace

- A personal agent can use both its own memory and the user's general knowledge
  and preferences. Organisation agents must not acquire an employee's personal
  memories by default.
- Memory persists across runs. A background process should consolidate useful
  user knowledge and agent-specific lessons, including pitfalls, so they can
  prevent repeated mistakes.
- Each agent has a persistent, isolated data area that it can reuse across runs
  for project work and its own structured information.
- The implementation should reuse user memories, document/canvas storage,
  agent state, and existing tool APIs where they fit. It must not expose
  arbitrary application database access to a model.

## Existing mechanisms to reuse

| Need | Existing AIplane mechanism | Gap to resolve |
|---|---|---|
| Agent definition and display name | `agents`, `AgentSpec`, drafts and published versions | Represent agent ownership and personal-vs-organisation execution mode |
| Identity and organization grants | system principals, `principal_grants`, agent shares | Personal agent must act with the user's authority while retaining agent attribution |
| Chat, runs and streaming | `chat_sessions`, `chat_turns`, `SessionDriver`, JSON-SSE | Associate a personal agent with user-owned sessions without turning it into a second chat engine |
| Durable pause and approval | `chat_turn_suspensions`, `agents::approval`, Inbox | Interactive chat and user-owned scheduled/webhook turns can persist a pause and resume through the user's Inbox. Agent execution must retain its attribution and re-evaluate current owner grants and agent rules when a paused turn resumes. |
| Schedules | `scheduled_actions` and scheduled worker | Personal agent binding and organization principal-owned, pinned-version execution are implemented; organization runs open fresh private sessions |
| Event triggers | Webhooks and headless execution | Personal agent binding and organization-owned asynchronous webhooks use durable assistant-turn correlation and current execution authority |
| MCP catalogue and execution | connector catalog, `McpConnectionManager`, `UserMcpLayer` | Personal delegated and agent-owned credentials; organisation/agent OAuth identity |
| Per-user memory | `user_memories`, memory tools, `agent_memories` | Runtime reads/writes and background curation for isolated agent memory |
| Agent conversation state | `agent_state` | Session-scoped today; agent workspace must persist independently across sessions |
| Browser and code execution | sandbox-runner, sandbox browser tools, browser-control | Persistent isolated computer/profile/storage per agent, suitable for background runs |
| Operator settings | declarative settings API and SPA | Agent creation permission and active-agent limit (default 4); define organization scope |
| Usage controls | user/agent spend and usage limits | Attribute background usage to its owner and agent; no separate parallel-run cap is requested |

### Shared mechanisms: personal agent access and approvals

The current code establishes these reuse points and the remaining runtime seam:

| Need | Existing mechanism | Agent-specific work |
|---|---|---|
| Resolve the signed-in user's current tool ceiling | `AppState::allowed_tools_for_session`, `UserMcpLayer`, and `CompositeToolSource` | Resolve the ceiling on each agent turn and compose only the connectors selected for that agent. Do not treat a tool name from the model as a grant. |
| Store an agent's tool allowlist and approval settings | The agent already has an `agents.draft_spec`; `AgentSpec.main.tools` and `main.tool_resources` represent tool selection and per-tool permission | Add an owner-scoped update/read path that validates against the user's current grants. The ordinary agent spec validator assumes grants belong to the agent principal, so it cannot be called with the agent's empty principal grants unchanged. Do not add a parallel action-policy table. |
| Enforce tool rules before execution | `RunToolSource`, `GrantedToolSource`, `Permissions` and `AskFirst` | `ChatPersona::with_tool_access` narrows the dynamic user/MCP source. `LivePersonalAgentTool` reloads active ownership, current user roles, selected rules, enabled settings and the actual connected tool before each invocation, then applies the existing approval wrapper. Current access also intersects the turn's initial grant ceiling. |
| Pause and resume a decision | `chat_turn_suspensions`, `AskFirst`, the user Inbox, and `resume_turn` | The chat resume path reloads the session's `personal_agent_sessions` link before constructing the driver. Keep that resolution and re-check the live owner grant and agent rule when resuming. |
| Attribute tool activity | `agent_audit` and the personal-agent attribution on `ToolContext` | Records the acting user, agent, tool, decision and outcome. Personal MCP admission records the currently selected typed connection and generations as the identity of the attempted call; outcomes remain correlated by the existing call ID. |

`Tool::changes_state()` is the existing default for agent approval and an MCP
tool reports a state change from its server's destructive and read-only
annotations. A personal agent needs a stricter default: missing or incomplete
read-only metadata must not silently turn an external action into an
unreviewed write. This stricter resolution belongs in the agent policy seam; it
must not change permission defaults for ordinary chat or existing agents.

The connector store represents mutually exclusive user/principal owners.
Personal agents can explicitly select the owner’s current connection or their
own authorized OAuth credential. Current organization member writers can
configure an agent’s own credential and run its saved draft or published version in a private principal
conversation. Organization schedules and asynchronous webhooks use retained
published versions and the same own-principal credentials. Public agent channels
remain unavailable. Ordinary principal runs use
explicitly granted shared-identity connectors. Existing `agent`-scope
connectors accept `none` or `static_bearer`. See [Agent identities](agents-identities.md)
for the OAuth selection and generation fences.

Gateway groups resolve identity-provider claims and resource rights. Actual
organization ownership is stored in `organizations`, group-derived membership
in `organization_groups`, and canonical `agent_owners` records carry exactly one User
or Organization owner. A group name is not an owner.

Personal agent execution retains the user principal and user-owned session for
authority, metering and private chat, while carrying the agent’s AgentRun for
its spec, own memory, activity chain and tool/action rules. Tools and models
use the intersection of those sources. Organization execution requires the
separate current principal authority bridge described in
[organization agents](agents-organizations.md).

Agent workspace files and browser profiles live outside per-turn containers in
the runner's validated persistent workspace. A `SandboxLease` still lives in
a turn's `ToolContext` and is released at turn end; the runner's lease table
has a configured TTL and capacity cap. The filesystem and browser profile are
mounted only for their typed agent workspace purpose, and browser handover keeps
the exact scoped container alive while its owner controls it.

The MCP manager’s ordinary overlays are: `layer_for_user` resolves
the user's allowed per-user OAuth connections plus allowed global connectors;
`layer_for_principal` resolves only granted connectors with a shared gateway
identity. The personal agent resolver applies its persisted identity selection, owner
grants and agent policy. Organization calls require their own selected-principal
resolver before execution is enabled. Exposing both
overlays wholesale and relying on the prompt to choose would exceed the
confirmed permission boundary.

Personal schedules and webhooks retain user ownership and optionally bind to a
personal agent through `personal_agent_id`. Organization schedules and asynchronous
webhooks use the owning principal and pinned published version. Their headless
workers open private conversations and resolve the same agent execution policy
as interactive runs. `tools_enabled` is an additional ceiling, not approval
for external writes. Webhook payloads retain their untrusted-input framing.

Personal memory has this split in the current code. `user_memories` is keyed only
by `user_id`; the four existing memory tools use the acting user's identity
and personal agent context to route writes into `agent_memories`. Agent
chats expose them only when the user's existing memory capability is granted
and enabled. `recall` combines both scopes and marks each result; correction
and deletion operate only on agent-scoped memories from an agent chat. The driver
injects the user's preferences only when the shared `recall` tool is granted
and enabled. Agent `agent_state` is a different store keyed by
conversation session and is used for typed orchestration slots, so it cannot
serve as persistent agent memory. A personal agent turn must get user-profile
memory under the user's existing memory grant and switch, plus a distinct
Agent-owned memory scope for lessons and project context. The default write
target must not silently turn every agent lesson into a user-wide preference.
Organization agent memory uses the same typed `agent_memories` accessors and
existing editor with exclusive agent/organization scopes. Its initialized principal
run reads own and shared knowledge; writes affect only its own notes. Current
run authority is checked in the same transaction as each note operation.

## Proposed architecture

```mermaid
flowchart TD
  UI[AIplane web: agents, chat, activity, approvals, workspace]
  API[/api/v0 JSON API]
  Domain[agent ownership and policy]
  Runtime[Existing agent run and chat runtime]
  MCP[Existing MCP catalog and connection manager]
  Jobs[Existing scheduler and webhook workers]
  Memory[User memories plus agent memory]
  Computer[Sandbox runner and isolated agent workspace]
  Audit[Existing agent activity log]
  UI --> API --> Domain --> Runtime
  Jobs --> Domain
  Runtime --> MCP
  Runtime --> Memory
  Runtime --> Computer
  Runtime --> Audit
```

The agent is a user-facing mode of AIplane's existing agent, not a replacement
for the agent runner. The execution context is explicit:

- `PersonalAgent { user_id, agent_id }`: tool access is resolved from the user's
  grants and connector identities, then narrowed by the agent's own policy.
- `OrganizationAgent { organization_id, principal_id }`: tool access is resolved
  only from the agent principal's grants and its own connector identities.

The concrete Rust type and schema are implementation choices. They must keep
the crate dependency direction intact and must not make `session-core` aware
of agent ownership.

The canonical `agent_owners` table stores the user-facing name and active state,
keyed by the existing agent principal with exactly one immutable user or
organization owner. User-facing names
are unique per owner, while the system principal retains a generated unique
internal name; two users can therefore each name an agent "Harald". Explicit
`organizations` metadata and selected `organization_groups` provide the
organization ownership and membership boundary. See [organization
Agents](agents-organizations.md) for the implemented configuration foundation and
remaining credential and execution requirements.

## Identity and authorization flow

```text
request from signed-in user
  -> load agent and verify owner/share access
  -> establish explicit agent execution context
  -> resolve owner permissions and chosen connector identity
  -> intersect owner permissions with agent tool/action policy
  -> apply hard safety rules and approval policy
  -> run the existing tool loop
  -> append activity as {actor user, agent, connector identity, action, result}
```

For personal agents, user authority is resolved at execution time so a revoked
user grant or disconnected connector takes effect without republishing the
Agent. For organisation agents, principal grants remain explicit. An agent cannot
use its model-visible instructions to change these decisions.

## Persistence concepts

These are logical responsibilities; table names and exact encoding are still
subject to migration design review.

- **Agent metadata:** existing agent definition plus owner type/id, active state,
  execution mode and agent-specific policy.
- **Connector identity selection:** per-agent connector choice of user identity,
  agent-owned identity or organisation-owned identity. Credentials remain
  encrypted and unavailable to model context.
- **User memory:** existing user-wide memory, readable by the user's personal
  agents only under the user's configured permission.
- **Agent memory:** separate scope for lessons and context owned by that agent.
- **Agent workspace:** persistent agent-owned project artifacts and structured
  data, separate from chat transcript and short-lived `agent_state`.
- **Work item/run correlation:** schedule or event source, agent, session, status,
  attempts, decisions and outcomes must be traceable through current runs and
  activity records. Avoid a new task table unless existing schedule/webhook
  records cannot represent a confirmed lifecycle requirement.

## Action policy

Policy is configured by an agent owner and evaluated for every tool call. It has
three outcomes: allow, require approval, or deny. Resolution must include the
owner's authority, agent policy, connector/tool policy and mandatory platform
rules. The most restrictive applicable result wins. Approval is durable and
uses the existing suspension and inbox flow.

Interactive chat and the existing user-owned scheduler/webhook workers can
persist a suspension in `chat_turn_suspensions`, release the worker and resume
the same tool call through the user's Inbox. Agent execution can reuse this
continuation when it keeps the agent context attached to the user-owned session
and verifies permissions again on resume. A tool that requires a decision
must still refuse to run on a path where nobody can resume it; enabling tools
on a schedule or webhook is not itself approval. Organization-owned runs need
an explicitly authorized approval owner and continuation path.

Action categories come from each tool's actual declared action catalogue.
Personal tool policies and approvals are implemented as described above.
Private organization decisions require current membership and Write access.
Responses preserve the original source, version, authored snapshot, model and
grant ceiling across restart. Published private decisions also appear in the
existing Inbox for current member writers. Publication does not admit
automation or external callers.

## Background work and memory curation

AIplane schedules and webhooks use the existing workers with an optional
personal agent binding. MCP calls may be used as event-driven work inputs where the connected
system supports them; the agent feature itself does not implement ticket-system
specific code.

OpenAI publicly describes background research as read-only and separately
describes scheduled tasks. OpenDots documents scheduled turns and says its
schedules are not a complete goal/event system. Neither source exposes a
general-purpose internal trigger implementation. AIplane will therefore use
its own scheduler and webhooks, with polling frequency, connector event
delivery and retry policy defined per supported deployment capability.

Memory curation should operate on existing conversations, user memory and
Agent memory through a bounded background run using the configured model
upstreams. It must not become model training. The curation cadence, evidence
retention, approval behavior and conflict resolution remain open decisions.

## User experience

The initial channel is the web SPA:

1. A user creates a named personal agent when the operator allows it and an
   active slot is available.
2. The user chooses model, tools, MCP connector identity and action rules.
3. The user chats with the agent, starts or edits ongoing work, and reviews
   activity and pending approvals.
4. The user can inspect and manage user-wide memory, agent memory and the agent's
   workspace.
5. The user can deactivate an agent to free an active slot.
6. An authorized organization manager creates an organization agent, gives it
   its own identity and grants only the required resources.

UI implementation requires a reviewed mockup/walkthrough before Svelte
changes, per the repository working agreement.

### Where personal agents appear

`/agents` lists personal agents under *My agents*, beside team agents and the
agents shared with the caller. Each card shows the agent's name, its kind and
the `active` state from its `ownership` metadata. **New agent** asks who the
agent is for; *Only me* creates a personal agent with
`POST /api/v0/agents` and `kind: "personal"`, and the server enforces the
active limit. The agent's editor at `/agents/{id}` has the **Chat** action
while it is active.

Selecting Chat creates a private session owned by the signed-in user,
associates it with the agent, and opens the existing `/chat/{session_id}`
experience. An inactive agent has no Chat action. Sessions remain private to
their user and agent, and deactivation prevents new messages from starting.
The session loads agent and user memory and applies the selected tool and
approval rules.

### Approved tool access and approvals walkthrough

The next personal-agent settings surface extends each existing agent card with an
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

Tool label and description from catalog                  agent rule             Identity
AIplane read-only tool from the user's catalog            Allow                AIplane
Read-only tool from connected Gira MCP                    Allow                My account
Write tool from connected Workspace MCP                  Ask before each call My account

Changes to these rules apply to new calls. Approval applies only to the pending
arguments. Current permissions and agent availability are checked again on resume.
```

The list contains only tool IDs returned by the user's allowed-tool resolution
and connected MCP layers; display names and descriptions come from their
existing catalogs. `Allow` and `Ask before each call` map to the existing
`AgentSpec.main.tools` and `main.tool_resources[tool].permission` fields.
Omitting a tool from `main.tools` denies it to that agent. The personal agent's
effective set remains the intersection of this agent selection and the user's
current grants and enabled settings. MCP `readOnlyHint` supplies the default
for read calls; a write or a tool without a read-only annotation defaults to
approval. The owner may change that default per tool within the user's grants.
The selected connector identity is resolved separately from this action rule;
an agent-owned identity is offered only after that connector has an agent-owned
credential.

An approval pauses the interactive agent turn through the existing
`AskFirst`/`chat_turn_suspensions`/Inbox path. Approve-once resumes the pending
call; a saved `Allow` rule changes future calls. Scheduled and webhook runs
persist approval-required calls and resume through the same Inbox path. The walkthrough does not add a parallel policy table or
approval mechanism. On resume, current ownership, active state and tool access
are checked again even when the call was approved once.

## Delivery plan

### Slice 1: Agent ownership and active lifecycle

- Establish user- and organization-owned agent records on top of existing agent
  definitions and principals.
- Let users create personal agents when their configured group permission
  allows it through the independent `can_manage_personal_agents` group flag.
- Enforce the configurable active personal agent cap (default 4) transactionally
  at creation/activation; deactivation frees capacity.
- Preserve current agent-manager workflows for visitor-facing and other
  principal-owned agents.
- Add the initial agent list/create/deactivate API and state-based tests.

### Slice 2: Delegated personal execution

- Carry explicit `PersonalAgent { user_id, agent_id }` attribution from the chat
  actor into `ToolContext`, while keeping the effective principal as the user.
  This establishes a scope for agent-owned data; it does not yet grant tools or
  enforce an agent action policy.
- Enforce user grants intersected with agent policy before offering or executing
  a tool.
- Attribute every action to user and agent in usage and audit.
- Keep sessions owner-scoped and private.

### Slice 3: MCP identity selection and action approval

- Reuse the MCP catalog, manager, OAuth refresh and secret sealing.
- Support selected user identity or agent-owned identity for personal agents.
- Support only agent-/organization-owned identity for organization agents.
- Add per-tool/action policy using the durable approval path.

### Slice 4: Persistent memory and workspace

- Use existing user memories for shared personal context.
- Add agent-specific memory scope and bounded background curation.
- Reuse document/canvas and sandbox storage where they meet workspace
  isolation and persistence requirements; otherwise design one narrowly scoped
  agent workspace store.

### Slice 5: Background work and isolated computer

- Extend scheduled and webhook execution to use agent context and activity.
- Establish safe retries, cancellation, concurrency and usage bounds.
- Extend sandbox-runner for a persistent isolated browser/filesystem per agent
  only after confirming the required lifecycle and deployment constraints.

### Slice 6: Organization management and operations

- Add organization ownership, manager access and organization/agent connector
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
- The confirmed default of four limits active personal agents, whether they are
  currently working or idle. The user has not requested a separate cap on
  simultaneous runs.

## Remaining implementation

The following adopted designs define the remaining implementation work:

- [Conversational setup and chat participation](agents-conversational-setup.md)
  records the requested natural-language agent creation and reconfiguration,
  guided prerequisites, organization peer delegation and ordinary-chat agent
  mentions. Its complex interaction mockup requires approval before UI work.
- [Organization agents](agents-organizations.md) defines an independent active
  cap, member/share approval standing and isolated principal execution.
- [Agent identities](agents-identities.md) defines the reused user OAuth account
  and explicitly selected own OAuth account. Personal selection and current
  organization member-writer credential management are implemented.
- [Memory curation](agents-memory-curation.md) defines source selection and
  reviewed optimization; its background job remains to be implemented.
- [Persistent workspace](agents-workspace.md) provides durable personal agent
  files and browser profiles on supported runner deployments. [Browser
  handover](agents-browser-handover.md) lets the owner control a suspended
  personal agent browser; organization handover and runner-replica coordination
  remain outside the implemented scope.
- Existing worker, run-budget and spend limits bound execution. The active
  agent cap counts enabled agents and does not impose a global parallel-run cap.

Deactivation is reversible and frees an active slot; this is the confirmed
product behavior and is implemented by the lifecycle API.

## Research references

- [Grok Bot feature comparison and additional product gaps](agents-grok-comparison.md)
- [xAI: Introducing Grok Bot](https://x.ai/news/introducing-grok-bot)
- [xAI: Grok Bot product page and FAQ](https://x.ai/bot)
- [OpenAI: Wir stellen dots vor](https://openai.com/de-DE/index/introducing-dots/)
- [OpenAI: How we build safety, security, and privacy into dots](https://openai.com/index/how-we-build-safety-security-and-privacy-into-dots/)
- [OpenAI Help Center: Getting started with your dot](https://help.openai.com/en/articles/20001530-getting-started-with-your-dot)
- [OpenAI Help Center: Dots privacy, security, and safety FAQs](https://help.openai.com/en/articles/20001529-dots-privacy-security-and-safety-faqs)
- [CopilotKit OpenDots repository README](https://github.com/CopilotKit/OpenDots/blob/main/README.md)
