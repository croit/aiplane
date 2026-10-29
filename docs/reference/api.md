# HTTP API reference

AIplane has separate interfaces for model clients, the browser application,
public agent visitors, agent-to-agent calls and webhook triggers. Credentials
for one interface do not automatically authorize another interface.

## Model clients

Use `Authorization: Bearer <token>` (the scheme name in any case) or `x-api-key: <token>`. Personal tokens
start with `gwk_`; system-principal tokens start with `gws_` and use their own
principal grants. Browser cookies do not authenticate `/v1` requests. The
gateway validates its credential and supplies the configured upstream key
when forwarding; client credentials are not passed to the model provider.

| Operation | Request | Result and prerequisite |
|---|---|---|
| `GET /v1/models` |
| `GET /v1/models/{*id}` |
| `POST /v1/chat/completions` |
| `POST /v1/responses` |
| `GET /v1/responses/{*id}` |
| `DELETE /v1/responses/{*id}` |
| `POST /v1/messages` |
| `POST /v1/messages/count_tokens` |
| `POST /v1/systemone` |
| `POST /v1/embeddings` |
| `POST /v1/rerank` |
| `POST /v1/images/generations` |
| `POST /v1/images/edits` |
| `POST /v1/audio/transcriptions` |
| `POST /v1/audio/speech` |
| `GET /v1/sandbox/files/{run}/{filename}` |

`/v1/responses` keeps a response for 30 days when the request has `store` on
(the default), so `previous_response_id` can continue it; only the person or
principal whose token created it can read, continue or delete it.

OpenAI-compatible upstreams determine many request options and response fields.
AIplane compatibility is endpoint-specific; an upstream's additional endpoint
is not automatically exposed by the gateway.

### Model facts

`GET /v1/models` and `GET /v1/models/{*id}` return OpenAI model objects with
additional fields. Every entry has `kind`, the pool kind that serves it
(`chat`, `transcription`, `embedding`, `image`, `speech`, `rerank` or
`system_one`). A chat model also carries, only where the gateway holds the
fact:

| Field | Source |
|---|---|
| `context_window` | The model's context window from model settings, else the window its backend reports. Compaction and the tool-output budget use the same value. |
| `max_output_tokens` | `max_completion_tokens` or `max_tokens` from the model's request defaults. |
| `capabilities.vision`, `capabilities.tools` | Capabilities an administrator set or the gateway learned from a backend refusal. |

An alias reports the facts of the model it resolves to. An automatic route
reports only its own settings. A field the gateway does not know is absent,
never filled with a guess.

```json
{"id":"Qwen/Qwen3.6-35B-A3B-FP8","object":"model","created":1792011760,
 "owned_by":"aiplane","kind":"chat","context_window":131072,
 "max_output_tokens":8192,"capabilities":{"vision":true,"tools":true}}
```

### OpenCode plugin

`GET /clients/opencode/aiplane.js` serves a plugin for OpenCode v2 with this
gateway's public URL filled in. It needs no sign-in and contains no
credential. Saved as `~/.config/opencode/plugins/aiplane.js`, it adds the
`aiplane` integration, which signs in with a personal `gwk_` token
(`opencode auth login aiplane`), and the `aiplane` provider, whose models are
the token's chat models from `/v1/models`. The plugin maps `context_window`,
`max_output_tokens` and `capabilities` onto OpenCode's model limits and
capabilities. Where the gateway reports no window it keeps OpenCode's own
fallbacks. The output limit never exceeds half the window, because OpenCode
reserves it inside the window. A model accepts images only when the gateway
reports `vision: true`; tools stay enabled unless it reports `tools: false`.
Models are reloaded when the stored token changes, and re-fetched on a five
minute tick — a model granted to the token, or a context window an operator
set, reaches the client without a restart. A failed tick keeps the last good
listing. OpenCode v1 is not supported.

### First completion and streaming

Set `AIPLANE_TOKEN` from your secret store and substitute an actually accessible
model ID. An OpenAI-compatible SDK uses `<AIplane-origin>/v1` as its base URL.

```bash
curl https://aiplane.example.com/v1/chat/completions \
  -H "Authorization: Bearer $AIPLANE_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"model":"your-model-id","messages":[{"role":"user","content":"Hello"}]}'
```

For streaming, add `"stream":true` to that JSON and use a streaming-capable
client (`curl -N` for a terminal). OpenAI chat uses `text/event-stream` with
`data:` frames and `[DONE]`. OpenAI Responses (`/v1/responses`) and Anthropic
Messages use their own named events.
Errors can occur after streaming headers have been sent; clients must inspect
stream events as well as the initial HTTP status.

### Server-side tools and client-owned tools

Token tool access must be enabled, and the tool must be registered, granted
and available in the caller's context. Built-in missing capability states
default to Auto; missing connector/skill states default to Off. Auto initially
offers tool discovery and adds selected schemas in later rounds. Off excludes
the capability; On offers it directly.

Gateway-owned calls execute inside AIplane and their results feed subsequent
model rounds. Client-owned calls are returned to the client to execute.
Chat-only tools require a browser conversation and are not offered through
the model API. Tools with a pending human-approval requirement cannot simply
execute through an unattended API request.

The gateway tool loop has a hard upstream-round budget (`MAX_TOOL_ROUNDS`, 16).
When the budget closes the turn, it requests a final answer from collected
results. A successful OpenAI response can include an additive `aiplane`
object with `tool_rounds` and `tool_budget_exhausted`; a stream includes this
signal in its finish chunk. A textless final/closing result can fail with
`tool_budget_exhausted`. Client-owned tool handoffs do not carry that signal.

### Routing and diagnostic headers

Aliases and automatic routes use the standard `model` field. The configured
model defaults fill omitted values; explicit client settings take precedence.
Pool/group permissions and a token's own model restriction are cumulative.

| Header | Meaning |
|---|---|
| `X-Gateway-Resolved-Model` | Effective model when it differs from the requested ID. |
| `X-Gateway-Backend` | Backend identity added by the routing response helper. |
| `X-Gateway-Tool-Rounds` | Upstream rounds for a buffered tool-loop completion. |
| `X-Gateway-Tool-Budget-Exhausted` | Budget closed the buffered completion turn. |
| `X-Gateway-Route-Alias` | Automatic route alias. |
| `X-Gateway-Route-Version` | Automatic route decision version. |
| `X-Gateway-Route-Reason` | Decision reason. |
| `X-Gateway-Route-Target` | Effective automatic-route target. |
| `X-Gateway-Route-Suggested-Target` | Suggested target where supplied by the decision. |
| `X-Gateway-Route-Confidence` | Confidence where supplied by the decision. |

Automatic-route session affinity accepts `X-Gateway-Session-Id` or
`X-Gateway-Session`. Reuse an identifier for the same client conversation.
Headers depend on the code path and decision; clients must tolerate absence.

### Reranking

`POST /v1/rerank` scores documents against a query with a model from a
`rerank` pool, the same backends knowledge search reranks with. The body is
relayed to the backend unchanged, so send the request in the backend's own
dialect. Cohere, Jina and vLLM accept this shape; Text Embeddings Inference,
for example, names the passages `texts` instead of `documents`:

```bash
curl https://aiplane.example.com/v1/rerank \
  -H "Authorization: Bearer $AIPLANE_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"model":"your-rerank-model","query":"refund policy","documents":["first passage","second passage"],"top_n":1}'
```

The gateway reads only `model`: it resolves aliases, rewrites `model` to the
real ID, forwards the body unchanged to the backend's `/rerank` and relays the
backend's response, so the response shape (for example `results[]` with
`index` and `relevance_score`) is the backend's.

Rerank models are ordinary models for access purposes, like embedding
models: `/v1/models` lists them, a token's model restriction can include
them, a system principal can be granted them, and the pool's group
restriction applies to the caller. A token restricted to other models gets
`403 model_not_allowed`; a system principal without a grant on the model gets
`404 model_not_found`, as does an unknown model. No healthy backend returns
`503`. Knowledge search reranks through the same pools as a gateway-internal
call: it is neither subject to the searching person's pool group restrictions
nor metered.

Every call is recorded as usage of kind `rerank` and counts against
request-count limits. Tokens are recorded only when the backend's response
carries a `usage` object with `prompt_tokens` or `total_tokens`; a bare
`total_tokens` (as vLLM reports) counts as input tokens, charged at the
model's input price when it is priced per token. A backend that reports no
usage (Text Embeddings Inference) or reports something other than tokens
(Cohere's `meta.billed_units.search_units`) is recorded with no tokens and no
cost, so its calls do not count against token quotas or spend limits.
`POST /v1/embeddings` follows the same rule: a bare `total_tokens` counts as
input tokens.

## Browser application API

`/api/v0` is the JSON API used by the SPA. Most operations require the signed
browser `id` session cookie. Handlers enforce additional owner, group,
administrator or agent-management permissions. A personal model API token is
not a replacement for a browser session.

Examples of its task families:

| Family | Purpose and documentation |
|---|---|
| `/api/v0/chat/*` | Conversation CRUD, messages, steering, cancellation, sharing, forking, files and documents; [chat](../guide/chat.md), [files](../guide/files-and-canvas.md). |
| `/api/v0/tokens*`, `/api/v0/me*` | Personal credentials, identity and preferences; [account guide](../guide/account-and-usage.md). |
| `/api/v0/memories*`, `/api/v0/tools*`, `/api/v0/integrations*`, `/api/v0/skills*` | Personal capabilities, connections and memories; [tools](../guide/tools-and-integrations.md). |
| `/api/v0/scheduled*`, `/api/v0/webhooks*`, `/api/v0/inbox*` | Automation and durable decisions; [automation guide](../guide/automation-and-inbox.md). |
| `/api/v0/agents*` | Agent drafts, grants, evaluation, publication and observation; agent guides. |
| `/api/v0/agents*` | List/create/active-state require resolved `can_manage_personal_agents`; owner-scoped access remains enforced. The four connection routes additionally admit organization member writers for explicit own-principal OAuth management; they return independent action capabilities and no user credential. The memory routes also admit organization member readers and writers through current membership and existing shares; shared-note writes additionally require gateway administration. The workspace routes retain personal ownership, including inactive agents. Listing returns actual storage availability, busy state and nullable metadata; raw downloads use attachment disposition, `nosniff` and `no-store`. Uploads and reads are capped at 50 MiB; writes, folder creation and removal require idle storage. Remaining routes retain personal ownership. Personal agent lifecycle, tool selection, tool/action approval rules, private chats, memory and activity; scheduled actions and webhooks can run as an owned agent; [AIplane agents](../personal-agents.md). |
| `/api/v0/organizations*` | Current group-derived membership, organization details and named organization agent creation/activation. Creation requires membership and agent-management permission; configuration uses existing agent shares and principal grants. Organization member writers can run private saved-draft and pinned published conversations through the existing agent test routes; [organization agents](../agents-organizations.md). |
| `/api/v0/rag*` | Knowledge collections, snapshots, indexing and profiles; [knowledge](../admin/knowledge.md). |
| `/api/v0/admin/*` | Administrative models, grants, settings and catalogs; administration guides. |
| `/api/v0/setup/*` | Verified setup/recovery state machine. |

`POST /api/v0/skills/draft` accepts `{name, goal, steps, stop_conditions, model}` from a signed-in user and returns `{manifest, model, saved:false}`. It requires personal-skill storage, validates the model against the caller's currently offered chat models, and meters generation to that user. It has no tool execution or persistence side effect. `POST /api/v0/skills` saves the reviewed manifest; JSON skill edits must set `overwrite:true`, while a create that collides with an existing personal skill returns `409`.

### Branding

The operator's company logo, set by an administrator under
[Settings → Branding](../admin/settings.md#branding). `POST /api/v0/admin/branding`
takes a multipart body with a `file` part (SVG or a raster image: PNG, JPEG,
WebP) and an optional `slot` part (`light`, the default, or `dark`); raster
uploads are resampled down to 1200 px on the longest side and stored as PNG,
SVG passes through byte-for-byte, and a stored image is refused above 512 KB
(`413 logo_too_large`). `GET /api/v0/admin/branding` reports which slots are
set, and `DELETE /api/v0/admin/branding/{slot}` drops one. `GET /brand/logo`
and `GET /brand/logo-dark` serve the stored bytes with their content type and
`Cache-Control: no-cache`; they are public because the SPA's brand lockup
renders on the login screen, and a missing slot is a plain 404 the SPA falls
back past to the built-in mark.

The running gateway supplies `/openapi.json`, a typed OpenAPI contract for the
registered `/api/v0` routes. It describes operations, path parameters, request
and response schemas, security requirements and the shared error envelope.
Use it as the machine-readable contract for the exact build you are calling.
Most `/api/v0` refusals return an `error` object with a readable `message` and
machine-readable `type` and `code`; validation responses may also include
field-level issues. Check each operation's OpenAPI response schema for details.

### Organization ownership and configuration

Administrators create and edit organization metadata through
`/api/v0/admin/organizations`. `member_groups` contains actual gateway-group
names. The list response's `member_group_user_counts` maps each bound group
name to its count of locally known users, resolved from each user's latest
stored OIDC claim values; default groups count every locally known user. It is
not a live IdP directory count. An empty list grants no membership. Updates require the opaque
`expected_revision` returned by the server; a stale revision returns `409`
without replacing metadata or group bindings.

Member organization lists contain actual memberships. Agent configuration access
intersects that membership with the existing user/group share and
agent-management permission. Membership alone does not expose an agent's
configuration. Respond-only shares supply existing Inbox standing rather than
the agent configuration response. Administrator recovery remains available
through the existing administration and agent-management surfaces.

Agent summaries carry nullable `ownership` metadata with the persisted name
and active state of a personal or team agent, beside its typed `owner`. An
agent's organization owner is immutable. The organization detail reports its
total active count and configured limit, while its agent list includes only
configurations the caller may read. Active idle agents count towards the
limit. Creation and activation enforce that limit in
the same transaction as the current authority check.

`POST /api/v0/agents` creates a personal agent for the caller when the body
carries `"kind": "personal"` (with `can_manage_personal_agents`; a `spec` is
refused, since a personal agent starts from its owner's tools) and a public
agent otherwise. A team agent is created with
`POST /api/v0/organizations/{id}/agents`. `GET` and `PUT
/api/v0/agents/{id}/communication` and `GET /api/v0/agents/{id}/activity`
answer by kind: for a personal agent its owner's consent with its peers and its
runs (paged with `cursor` and `limit`; the decision-trail filters,
`/activity/export` and `/activity/verify` apply to the other kinds), for any
other agent the callers it accepts and its decision trail.

`GET /api/v0/agent-resources?agent_id={id}` resolves the actual target agent's
configuration inventory. An organization connector is offered and grantable
only after a current member writer explicitly selects a connected own-principal
OAuth credential with current catalogue, credential and binding generations.
The writer's existing catalogue eligibility and grant ceiling also apply;
a selected credential is separate from the principal's Connector grant.

`POST /api/v0/agents/{id}/test/messages` requires
`{message, source:"draft"|"published", session_id?}`. Ordinary agents accept
explicit `draft`; active organization agents additionally accept `published` for
current member writers. A new published conversation uses the actual current
live version. Continuing it keeps its stored positive version after publishing
or rollback; changing source requires a new conversation. The response is
`202 {session_id, turn_id, source, agent_version}`. Draft uses version `0`.
No client-supplied version or organization ID selects authority.

Organization conversations are private principal-owned sessions with
`user_id=NULL`, `shared=0`, no visitor and no parent turn. Events and debug
require current member Write access and exact private source/version proof.
Debug also returns `source` and `agent_version`. Administrator recovery
without membership does not admit execution or history.

Staff resume rechecks current member Write access in its atomic claim
transaction. Strict saved authority retains the original source, authored
snapshot, resolved model and initial principal grant/model-pool ceilings across
restart. Published snapshots must agree with their actual version row; a cache
hit cannot replace that proof. Current selected MCP account facts must match.
A refused claim preserves the waiting request. New publication, draft edits or
added grants cannot enlarge the paused run. Internal run context and secrets
are absent from the public suspension response.

Published private approvals, human answers and secure input appear in the
existing Inbox only for current member writers. Draft pauses remain inline.
Read, Respond and administrator recovery without membership expose no private
items. Organization announcements do not send push or external channel messages.
Publication alone does not admit public, visitor, embedded, delegated or
test-suite execution. Scheduled actions and webhooks require their own retained
published pin and exact recorded private-run admission.

Organization share suggestions use the existing eight-result cap and check
current membership before including users. A search examines at most one
thousand matching candidates. When more candidates remain at that boundary,
`truncated` is `true`; narrow the query before relying on the result. Group
suggestions contain the organization's actual bound groups.

### Agent memory scopes

`GET /api/v0/agents/{id}/memories` returns at most 500 merged notes and a
`scopes` array with each actual scope's `scope`, configured `name` and current
`writable` flag. Each note includes its `scope`. Personal agents expose `agent`;
organization agents expose `agent` and `organization`. The server derives the
organization from the target agent, never from an input organization ID.

POST and PUT require `{kind,content,scope}`; DELETE requires `?scope=agent` or
`?scope=organization`. Missing or invalid scope returns 400. New and updated
content is limited to 2,000 UTF-8 bytes; existing longer notes remain readable.
Kinds are `preference`, `project` and `fact`.

Organization reads require current membership, agent-management permission
and effective Read or Write access to that agent. Own-note changes require
Write; shared organization-note changes also require current gateway
administration rights. A narrowly shared writer receives 403 on shared
mutation. Respond-only and nonmember recovery access expose no notes. The
server rechecks authority and applies each operation in the same transaction;
returned writable flags do not authorize a later request. Foreign note IDs
return 404 without revealing content.

### Personal agent browser handover

An active personal agent can request owner help through its existing `browse_page`
tool. The exact chat turn pauses while the owner uses the agent's server-side
browser in the web conversation. The five `browser-handover` routes in the
operation index claim one controller, return private JPEG frames, accept typed
input, return control to the same turn, or stop that run. All require the same
authenticated owner session and same-origin `Origin`; responses use
`Cache-Control: no-store, private`. The claim body carries a client-generated
`controller_id`; retrying with that same ID is idempotent, while another
controller is refused. Frames and input are never added to model messages or
chat attachments. This flow is personal-agent-only and uses the runner's
persistent browser profile when persistent storage is available; see [browser handover](../agents-browser-handover.md)
and [workspace deployment limits](../agents-workspace.md#operator-implications).

### Conversation events

Submitting a message and subscribing to its events are separate operations.
The browser posts JSON to the conversation's messages endpoint and reads the
conversation's `events` endpoint as an SSE stream. Its data frames use the
AIplane JSON event protocol (`snapshot`, `turn_delta`, tool and state events),
not OpenAI chat chunks. Reconnect using the existing protocol's snapshot and
event handling; do not append a reconnect snapshot as if it were new text.

### Personal agent automation

Schedule and webhook create/update bodies accept
`personal_agent_id: string | null`, the personal agent the run is bound to
(`agent_version` belongs to an organization agent's `principal_id`).
Omitting it selects ordinary user execution. A changed binding must select an
active owned agent; an unchanged unavailable binding can be retained while other
configuration is edited, but cannot execute. Responses include the stored
`personal_agent_id` and, as `agent`, the actual owned agent metadata when that
agent exists.

`linked_session_id` is valid only with conversation reuse. Omission retains
the link, an empty string clears it, and a concrete ID must identify a private
owned conversation with the same agent binding. Changing the agent binding clears
the existing link unless a matching replacement is supplied. The chat-session
list accepts `agent_id`: absence lists all conversations, an empty value selects
ordinary conversations, and an agent ID selects that owner's agent conversations.
This scope also applies to text search before its result limit.

Run history includes the exact `assistant_turn_id`. A synchronous webhook or
manual rerun that waits for approval returns `202` with status `waiting` and
its conversation ID. Decisions use the existing Inbox and resume the recorded
turn with its original history limit and tool ceiling. Disabling a trigger
stops new firings while allowing a pending decision to settle; deleting a
trigger with a nonterminal run returns `409`. Running background turns can be
watched and stopped through the existing conversation endpoints; interactive
messages, steering and history rewrites are refused while that turn runs.

### Organization scheduled actions

`GET /api/v0/scheduled?principal_id={agent_id}` selects an organization agent's
schedule inventory. Without that query, the route lists the authenticated
user's schedules. The scoped response contains `principal_id`, `can_write`,
`actions`, the actual target in `agent_owners`, and its published `versions` using the
existing agent version shape. Organization responses have `models: []` and
`default_timezone: null`; the client must supply a timezone. An empty schedule
list still returns the actual agent and its versions.

Creation uses the existing `POST /api/v0/scheduled` route. For an organization
Agent, send `principal_id`, a positive `agent_version`, `name`, `prompt`, `cron`,
`timezone`, `tools_enabled`, `reuse_conversation: false` and `reuse_rounds`.
Omit `model`, `agent_id` and `linked_session_id`. The model comes from the pinned
published spec, or the existing gateway Chat default when the spec omits it,
under the agent principal's current grants. Each fire opens a fresh private principal-owned
conversation. Organization conversation reuse is unavailable.

The returned action includes `principal_id`, `agent_version` and
`configuration_generation`; its `model` and `agent_id` are null. Ownership is
immutable. The schedule belongs to the principal and survives deletion of its
creating user. Inventory, creation, edits, toggles, deletion and run history
require current organization membership and effective Write access, checked in
the operation's transaction. Nonmember administrator recovery rights do not
expose these private surfaces. Personal agent management permission is not a
prerequisite.

`PUT /api/v0/scheduled/{id}` requires the complete configuration and
`expected_configuration_generation` from the returned action. Organization
updates retain the same `principal_id`. `POST /api/v0/scheduled/{id}/toggle`
requires `{enabled,expected_configuration_generation}` and returns
`{enabled,configuration_generation}`. These generation requirements also apply
to user-owned schedules. Missing fields are rejected; stale generations return
409. Execution configuration changes rotate the generation. Identical saves,
name-only changes and pause/resume retain it.

`GET /api/v0/scheduled/{id}/runs` derives scope from the stored action; no
principal query is required. It returns the actual action and up to 50 runs,
including `session_id`, `assistant_turn_id`, `agent_version` and
`configuration_generation`. Older user runs may have null version and
generation snapshots. The organization UI opens a recorded conversation at
`/agents/{principal_id}?tab=try&test_session={session_id}`. Its source and pinned
version are obtained from the private history endpoints.

Pausing prevents new fires and preserves an admitted pending decision.
Changing execution configuration invalidates a pending run's former dispatch
authority. To stop a running or suspended private turn, use
`POST /api/v0/agents/{id}/test/{session}/cancel` with `{turn_id}`. The exact
assistant turn must belong to that private conversation; current member Write
access is required. Cancellation remains available after deactivation,
principal disablement, grant removal or configuration changes. Success returns
`{ok:true}`; an unavailable turn returns 404. Ordinary agents can use this route
for their private draft conversations.

Deleting a schedule with a running, waiting or starting run returns 409.
Stop or settle it before retrying. Deleting an agent with retained principal-owned
schedules also returns 409: delete those schedules first, even if their runs
have finished. Schedule deletion removes its recorded run history.

### Organization webhooks

`GET /api/v0/webhooks?principal_id={agent_id}` selects an organization agent's
webhook inventory. Without this query, it returns the authenticated user's
webhooks. The scoped response contains `principal_id`, `can_write`, `webhooks`,
the actual target in `agent_owners` and its retained published `versions`.

`POST /api/v0/webhooks` accepts `principal_id`, positive `agent_version`, `name`,
`prompt` and `tools_enabled`. Organization sources require asynchronous mode
and fresh conversations: omit `model`, `agent_id` and `linked_session_id`, and
keep `synchronous` and `reuse_conversation` false. The response contains
`{webhook,secret}`; only this creation and rotation disclose the minted secret.
The stored webhook includes `principal_id`, `agent_version` and its nonempty
`configuration_generation`. Its model and personal agent binding are null.
Ownership is immutable; creator deletion does not delete the principal's source.

Current member Write guards inventory, configuration, secret rotation, deletion,
history and replay. `PUT /api/v0/webhooks/{id}` requires the configuration plus
`expected_configuration_generation`. Toggle requires
`{enabled,expected_configuration_generation}`; rotation requires
`{expected_configuration_generation}`. Replay requires
`{prompt,expected_configuration_generation,run?}`. These generation requirements
also apply to user-owned webhooks. Missing or incorrectly typed required fields
return 400; stale generations return 409. Effective execution changes rotate the
generation; names, unchanged saves, pausing and secret rotation preserve it.

`GET /api/v0/webhooks/{id}/runs` returns `{runs}` scoped by the stored owner.
Each new run includes its exact assistant turn, source generation and authored
version; legacy user snapshots remain null. Organization history opens the same
private agent Test chat as scheduled runs. Principal grants, selected own
credentials, approvals and usage limits govern execution.

Public `GET` or `POST /hooks/{secret}` returns `202 {session_id}` for organization
sources, without output, private errors or waiting decisions. The existing
256 KiB input-prefix limit truncates larger bodies. Unknown, rotated and paused
secrets return the same 404. Final admission rechecks the original secret,
enabled state and generation after reading the body.

An explicit member-writer replay may run while the source is paused. It uses
actual stored input and the submitted prompt in a new private conversation,
under the current source's retained publication. Membership and Write are checked
again before returning its private result. Rotation and pause preserve admitted
decisions; execution edits invalidate further dispatch from older generations.
Exact Stop uses the private cancellation endpoint described above. Pending or
starting runs prevent source deletion; retained sources prevent agent deletion.

## Public entry points and credentials

- `/hooks/{secret}` accepts GET/POST webhook triggers; the URL secret is its
  credential. Acceptance is not proof that the resulting run succeeded.
- `/hooks/rag/{token}` triggers the corresponding collection sync.
- `/api/v0/embed/*` uses embed keys and scoped visitor sessions. An embed key's
  origin allowlist and an agent's published access determine availability.
- `/a2a/agents/*` exposes the enabled published agent's A2A surface. Caller
  authentication and the caller system principal's `a2a_caller` grant for that agent apply to calls. Personal API tokens cannot make those calls.
- `/auth/*`, RAG OAuth and connector OAuth routes complete browser provider
  flows. They are not generic bearer-authenticated model endpoints.
- `/healthz` is public liveness. `/readyz` is public setup readiness: `503`
  with `setup_required` before completion, `200` afterwards.
- `/metrics` is the Prometheus scrape endpoint. Until an administrator
  switches it on and sets a scrape token, an allowed IP list or both, it
  answers like a path that does not exist; see
  [monitoring](../operations/monitoring.md).

See [agent publication](../agent-guide/test-publish.md) for the widget snippet,
identity modes, origins, A2A configuration and their limitations.

## Errors, limits and body sizes

| Status | Typical condition | What to inspect |
|---|---|---|
| `400` | Malformed request or invalid operation arguments | Body and the handler's reported validation error. |
| `401` | Missing, expired, revoked or invalid credential/session | Credential type for this interface. |
| `403` | Explicit model restriction or permission denial | Credential and principal grants/policies. |
| `404` | Missing resource or undiscoverable model | Actual IDs and accessible pools/resources. |
| `409` | Operation conflicts with current resource state, such as deleting an active token or repeating a completed revocation | Resource state and the error code/message. |
| `413` | Request body cap exceeded | Route's upload/body cap. |
| `422` | Request is readable but fails a resource-specific validation contract, such as an agent specification | Returned error code and validation issues. |
| `429` | Rate, token quota or spend limit exceeded | Applicable global/group/user/token rule and period. |
| `502` | Upstream failure | Error code, backend configuration and logs. |
| `503` | Required service or feature is not configured/available, or no upstream capacity is available | Error code, feature prerequisites, backend/service configuration and logs. |

OpenAI model paths generally use an `error` object with message/type/code;
Anthropic paths use the Anthropic error dialect. Do not assume every
download/OAuth endpoint has the same envelope. Sandbox-file errors, for
example, are plain text.

Most route bodies are capped at 1 MiB. The upload group, including model
requests, attachments and selected archives, is capped at 64 MiB. Public
handler-capped entry points have their own tighter limits. Proxies and
ingresses may impose additional caps. Usage records upstream work, so an
agentic request can produce multiple metered exchanges.

### HTTP endpoints

The index lists the exact methods and paths accepted by this build.
Debug fixture routes exist only in development builds.

| Operation |
|---|
| `GET /` |
| `GET /__dev/seed-session` |
| `GET /__dev/session` |
| `POST /a2a/agents/{id}` |
| `GET /a2a/agents/{id}/agent-card.json` |
| `GET /brand/logo` |
| `GET /brand/logo-dark` |
| `HEAD /api/hello` |
| `GET /api/v0/admin/automatic-routes` |
| `PUT /api/v0/admin/automatic-routes` |
| `DELETE /api/v0/admin/automatic-routes/{*alias}` |
| `PUT /api/v0/admin/backends` |
| `POST /api/v0/admin/backends/test` |
| `DELETE /api/v0/admin/backends/{name}` |
| `POST /api/v0/admin/backends/{name}/enabled` |
| `POST /api/v0/admin/backends/{name}/rename` |
| `GET /api/v0/admin/branding` |
| `POST /api/v0/admin/branding` |
| `DELETE /api/v0/admin/branding/{slot}` |
| `GET /api/v0/admin/connectors` |
| `PUT /api/v0/admin/connectors` |
| `POST /api/v0/admin/connectors/restore-defaults` |
| `DELETE /api/v0/admin/connectors/{key}` |
| `GET /api/v0/admin/connectors/{key}/audit` |
| `POST /api/v0/admin/connectors/{key}/toggle` |
| `GET /api/v0/admin/groups` |
| `PUT /api/v0/admin/groups` |
| `DELETE /api/v0/admin/groups/{name}` |
| `POST /api/v0/admin/impersonate/stop` |
| `GET /api/v0/admin/limits` |
| `POST /api/v0/admin/limits` |
| `DELETE /api/v0/admin/limits/{id}` |
| `PUT /api/v0/admin/model-defaults` |
| `GET /api/v0/admin/models` |
| `PUT /api/v0/admin/models` |
| `DELETE /api/v0/admin/models/{name}` |
| `GET /api/v0/admin/organizations` |
| `POST /api/v0/admin/organizations` |
| `PUT /api/v0/admin/organizations/{id}` |
| `PUT /api/v0/admin/pools` |
| `DELETE /api/v0/admin/pools/{name}` |
| `POST /api/v0/admin/pools/{name}/rename` |
| `PUT /api/v0/admin/search-settings` |
| `GET /api/v0/admin/settings` |
| `POST /api/v0/admin/settings` |
| `POST /api/v0/admin/settings/clear` |
| `GET /api/v0/admin/skills` |
| `POST /api/v0/admin/skills` |
| `PUT /api/v0/admin/skills/grants` |
| `DELETE /api/v0/admin/skills/{name}` |
| `GET /api/v0/admin/skills/{name}/archive` |
| `GET /api/v0/admin/tokens` |
| `PUT /api/v0/admin/tokens/{id}/models` |
| `PUT /api/v0/admin/tokens/{id}/priority` |
| `GET /api/v0/admin/upstreams` |
| `GET /api/v0/admin/upstreams/events` |
| `PUT /api/v0/admin/upstreams/fallback` |
| `POST /api/v0/admin/upstreams/reload` |
| `GET /api/v0/admin/users` |
| `POST /api/v0/admin/users/{id}/impersonate` |
| `POST /api/v0/agent-architect` |
| `GET /api/v0/agent-architect/{session_id}/agent-plan` |
| `POST /api/v0/agent-architect/{session_id}/agent-plan` |
| `GET /api/v0/agent-resources` |
| `GET /api/v0/agents` |
| `POST /api/v0/agents` |
| `GET /api/v0/organizations` |
| `GET /api/v0/organizations/{id}` |
| `POST /api/v0/organizations/{id}/agents` |
| `POST /api/v0/organizations/{id}/agents/{agent}/active` |
| `PUT /api/v0/agents/{id}/active` |
| `GET /api/v0/agents/{id}/profile` |
| `PUT /api/v0/agents/{id}/profile` |
| `GET /api/v0/agents/{id}/access` |
| `PUT /api/v0/agents/{id}/access` |
| `GET /api/v0/agents/{id}/connections` |
| `PUT /api/v0/agents/{id}/connections/{key}` |
| `POST /api/v0/agents/{id}/connections/{key}/connect` |
| `DELETE /api/v0/agents/{id}/connections/{key}` |
| `GET /api/v0/agents/{id}/memories` |
| `POST /api/v0/agents/{id}/memories` |
| `PUT /api/v0/agents/{id}/memories/{memory_id}` |
| `DELETE /api/v0/agents/{id}/memories/{memory_id}` |
| `GET /api/v0/agents/{id}/workspace` |
| `GET /api/v0/agents/{id}/workspace/file` |
| `PUT /api/v0/agents/{id}/workspace/file` |
| `POST /api/v0/agents/{id}/workspace/folder` |
| `DELETE /api/v0/agents/{id}/workspace/entry` |
| `POST /api/v0/agents/{id}/sessions` |
| `GET /api/v0/agents/inbox` |
| `GET /api/v0/agents/inbox/events` |
| `POST /api/v0/agents/inbox/{id}/answer` |
| `DELETE /api/v0/agents/{id}` |
| `GET /api/v0/agents/{id}` |
| `GET /api/v0/agents/{id}/activity` |
| `GET /api/v0/agents/{id}/activity/export` |
| `GET /api/v0/agents/{id}/activity/verify` |
| `GET /api/v0/agents/{id}/analytics` |
| `POST /api/v0/agents/{id}/assist/improve` |
| `POST /api/v0/agents/{id}/assist/suggest` |
| `GET /api/v0/agents/{id}/channels` |
| `POST /api/v0/agents/{id}/channels` |
| `DELETE /api/v0/agents/{id}/channels/{channel_id}` |
| `GET /api/v0/agents/{id}/communication` |
| `PUT /api/v0/agents/{id}/communication` |
| `POST /api/v0/agents/{id}/conversations/{session}/turns/{turn}/resume` |
| `PUT /api/v0/agents/{id}/draft` |
| `POST /api/v0/agents/{id}/draft/restore` |
| `GET /api/v0/agents/{id}/embed-keys` |
| `POST /api/v0/agents/{id}/embed-keys` |
| `POST /api/v0/agents/{id}/embed-keys/{key_id}/revoke` |
| `POST /api/v0/agents/{id}/live` |
| `POST /api/v0/agents/{id}/publish` |
| `GET /api/v0/agents/{id}/share-subjects` |
| `GET /api/v0/agents/{id}/shares` |
| `POST /api/v0/agents/{id}/shares` |
| `POST /api/v0/agents/{id}/shares/revoke` |
| `GET /api/v0/agents/{id}/test-runs` |
| `GET /api/v0/agents/{id}/test-runs/{run}` |
| `POST /api/v0/agents/{id}/test/messages` |
| `GET /api/v0/agents/{id}/test/{session}/events` |
| `POST /api/v0/agents/{id}/test/{session}/cancel` |
| `GET /api/v0/agents/{id}/test/{session}/turns/{turn}/debug` |
| `GET /api/v0/agents/{id}/tests` |
| `POST /api/v0/agents/{id}/tests` |
| `POST /api/v0/agents/{id}/tests/run` |
| `DELETE /api/v0/agents/{id}/tests/{case}` |
| `PUT /api/v0/agents/{id}/tests/{case}` |
| `GET /api/v0/agents/{id}/versions` |
| `GET /api/v0/build` |
| `GET /api/v0/chat/attachment/{turn_id}/{filename}` |
| `GET /api/v0/chat/landing` |
| `GET /api/v0/chat/sessions` |
| `POST /api/v0/chat/sessions` |
| `DELETE /api/v0/chat/sessions/{id}` |
| `GET /api/v0/chat/sessions/{id}` |
| `POST /api/v0/chat/sessions/{id}/cancel` |
| `POST /api/v0/chat/sessions/{session}/turns/{turn}/browser-handover/{request}/claim` |
| `POST /api/v0/chat/sessions/{session}/turns/{turn}/browser-handover/{request}/frame` |
| `POST /api/v0/chat/sessions/{session}/turns/{turn}/browser-handover/{request}/input` |
| `POST /api/v0/chat/sessions/{session}/turns/{turn}/browser-handover/{request}/finish` |
| `POST /api/v0/chat/sessions/{session}/turns/{turn}/browser-handover/{request}/abort` |
| `GET /api/v0/chat/sessions/{id}/capabilities` |
| `POST /api/v0/chat/sessions/{id}/capabilities` |
| `POST /api/v0/chat/sessions/{id}/confidential` |
| `GET /api/v0/chat/sessions/{id}/documents` |
| `GET /api/v0/chat/sessions/{id}/documents/{doc_id}` |
| `PUT /api/v0/chat/sessions/{id}/documents/{doc_id}` |
| `POST /api/v0/chat/sessions/{id}/effort` |
| `GET /api/v0/chat/sessions/{id}/events` |
| `GET /api/v0/chat/sessions/{id}/export.md` |
| `GET /api/v0/chat/sessions/{id}/export.pdf` |
| `POST /api/v0/chat/sessions/{id}/fork` |
| `POST /api/v0/chat/sessions/{id}/messages` |
| `POST /api/v0/chat/sessions/{id}/pin` |
| `POST /api/v0/chat/sessions/{id}/share` |
| `POST /api/v0/chat/sessions/{id}/steer` |
| `POST /api/v0/chat/sessions/{id}/steer/{steer_id}/discard` |
| `DELETE /api/v0/chat/sessions/{id}/turns/{turn_id}` |
| `DELETE /api/v0/chat/sessions/{id}/turns/{turn_id}/attachments/{filename}` |
| `POST /api/v0/chat/sessions/{id}/turns/{turn_id}/edit` |
| `POST /api/v0/chat/sessions/{id}/turns/{turn_id}/resume` |
| `POST /api/v0/chat/sessions/{id}/turns/{turn_id}/retry` |
| `GET /api/v0/comfyui/catalog` |
| `GET /api/v0/comfyui/health` |
| `POST /api/v0/comfyui/reload` |
| `POST /api/v0/embed/agent` |
| `GET /api/v0/embed/events` |
| `POST /api/v0/embed/identity` |
| `POST /api/v0/embed/messages` |
| `GET /api/v0/embed/recorder.js` |
| `POST /api/v0/embed/resume` |
| `GET /api/v0/embed/session` |
| `POST /api/v0/embed/sessions` |
| `POST /api/v0/embed/speak` |
| `POST /api/v0/embed/transcribe` |
| `POST /api/v0/feedback` |
| `GET /api/v0/feedback/config` |
| `POST /api/v0/feedback/extract` |
| `GET /api/v0/integrations` |
| `POST /api/v0/integrations/{key}/disconnect` |
| `POST /api/v0/integrations/{key}/retry` |
| `POST /api/v0/integrations/{key}/token` |
| `POST /api/v0/integrations/{key}/tools/all` |
| `POST /api/v0/integrations/{key}/tools/mode` |
| `GET /api/v0/me` |
| `POST /api/v0/me/ask/feedback/{turn_id}` |
| `POST /api/v0/me/browser/feedback/{turn_id}` |
| `DELETE /api/v0/me/location` |
| `POST /api/v0/me/location` |
| `POST /api/v0/me/location/feedback/{turn_id}` |
| `POST /api/v0/me/speech_voice` |
| `POST /api/v0/me/timezone` |
| `GET /api/v0/memories` |
| `POST /api/v0/memories` |
| `DELETE /api/v0/memories/{id}` |
| `PUT /api/v0/memories/{id}` |
| `GET /api/v0/models` |
| `GET /api/v0/push/config` |
| `POST /api/v0/push/subscribe` |
| `POST /api/v0/push/unsubscribe` |
| `GET /api/v0/rag/collections` |
| `POST /api/v0/rag/collections` |
| `DELETE /api/v0/rag/collections/{id}` |
| `GET /api/v0/rag/collections/{id}` |
| `PATCH /api/v0/rag/collections/{id}` |
| `GET /api/v0/rag/collections/{id}/refs` |
| `POST /api/v0/rag/collections/{id}/refs` |
| `DELETE /api/v0/rag/collections/{id}/refs/{ref_id}` |
| `PATCH /api/v0/rag/collections/{id}/refs/{ref_id}` |
| `GET /api/v0/rag/collections/{id}/refs/{ref_id}/log` |
| `POST /api/v0/rag/collections/{id}/refs/{ref_id}/primary` |
| `POST /api/v0/rag/collections/{id}/refs/{ref_id}/rebuild` |
| `POST /api/v0/rag/collections/{id}/reindex` |
| `POST /api/v0/rag/collections/{id}/sync-token` |
| `POST /api/v0/rag/collections/{id}/sync-token/clear` |
| `GET /api/v0/rag/profiles` |
| `POST /api/v0/rag/profiles` |
| `DELETE /api/v0/rag/profiles/{name}` |
| `PUT /api/v0/rag/profiles/{name}` |
| `GET /api/v0/rag/providers` |
| `POST /api/v0/rag/test-source` |
| `GET /api/v0/scheduled` |
| `POST /api/v0/scheduled` |
| `POST /api/v0/scheduled/preview` |
| `DELETE /api/v0/scheduled/{id}` |
| `PUT /api/v0/scheduled/{id}` |
| `GET /api/v0/scheduled/{id}/runs` |
| `POST /api/v0/scheduled/{id}/toggle` |
| `POST /api/v0/setup/finish` |
| `POST /api/v0/setup/restart` |
| `GET /api/v0/setup/state` |
| `POST /api/v0/setup/test` |
| `GET /api/v0/skills` |
| `POST /api/v0/skills` |
| `POST /api/v0/skills/draft` |
| `DELETE /api/v0/skills/{name}` |
| `GET /api/v0/skills/{name}/archive` |
| `GET /api/v0/skills/{name}/body` |
| `POST /api/v0/speech` |
| `GET /api/v0/system-principals` |
| `POST /api/v0/system-principals` |
| `GET /api/v0/system-principals/{id}` |
| `POST /api/v0/system-principals/{id}/disable` |
| `POST /api/v0/system-principals/{id}/grants` |
| `POST /api/v0/system-principals/{id}/grants/revoke` |
| `POST /api/v0/system-principals/{id}/tokens` |
| `POST /api/v0/system-principals/{id}/tokens/{token_id}/revoke` |
| `GET /api/v0/tokens` |
| `POST /api/v0/tokens` |
| `GET /api/v0/tokens/details` |
| `DELETE /api/v0/tokens/{id}` |
| `PUT /api/v0/tokens/{id}/mcp-policy` |
| `PUT /api/v0/tokens/{id}/models` |
| `PUT /api/v0/tokens/{id}/priority` |
| `POST /api/v0/tokens/{id}/quota` |
| `DELETE /api/v0/tokens/{id}/quota/{rule_id}` |
| `POST /api/v0/tokens/{id}/revoke` |
| `POST /api/v0/tokens/{id}/rotate` |
| `PUT /api/v0/tokens/{id}/tools` |
| `GET /api/v0/tools` |
| `POST /api/v0/tools/toggle` |
| `GET /api/v0/transcription_models` |
| `POST /api/v0/transcriptions` |
| `GET /api/v0/usage` |
| `GET /api/v0/webhooks` |
| `POST /api/v0/webhooks` |
| `DELETE /api/v0/webhooks/{id}` |
| `PUT /api/v0/webhooks/{id}` |
| `POST /api/v0/webhooks/{id}/rerun` |
| `POST /api/v0/webhooks/{id}/rotate` |
| `GET /api/v0/webhooks/{id}/runs` |
| `POST /api/v0/webhooks/{id}/toggle` |
| `GET /auth/callback` |
| `GET /auth/login` |
| `POST /auth/logout` |
| `GET /chat/attachment/{turn_id}/{filename}` |
| `GET /clients/opencode/aiplane.js` |
| `GET /healthz` |
| `POST /hooks/rag/{token}` |
| `GET /hooks/{secret}` |
| `POST /hooks/{secret}` |
| `GET /integrations/callback` |
| `POST /integrations/{key}/connect` |
| `POST /integrations/{key}/retry` |
| `GET /metrics` |
| `GET /openapi.json` |
| `GET /rag/oauth/callback` |
| `GET /rag/{id}/connect` |
| `GET /readyz` |
| `POST /v1/audio/speech` |
| `POST /v1/audio/transcriptions` |
| `POST /v1/chat/completions` |
| `POST /v1/embeddings` |
| `POST /v1/images/edits` |
| `POST /v1/images/generations` |
| `POST /v1/messages` |
| `POST /v1/messages/count_tokens` |
| `GET /v1/models` |
| `GET /v1/models/{*id}` |
| `POST /v1/rerank` |
| `POST /v1/responses` |
| `GET /v1/responses/{*id}` |
| `DELETE /v1/responses/{*id}` |
| `GET /v1/sandbox/files/{run}/{filename}` |
| `POST /v1/systemone` |
| `GET /{*name}` |

Admin token priority accepts `{"priority":"high"}`, `"medium"`, `"low"` or `null` (inherit). It requires an administrator session and returns the saved priority. Group and backend save schemas include their priority settings. See [Request priority](../request-priority.md).
