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
| `/api/v0/dots*` | List/create/active-state require resolved `can_manage_dots`; owner-scoped access remains enforced. Personal Dot lifecycle, tool selection, tool/action approval rules, private chats, memory and activity; scheduled actions and webhooks can run as an owned Dot; [AIplane Dots](../dots.md). |
| `/api/v0/rag*` | Knowledge collections, snapshots, indexing and profiles; [knowledge](../admin/knowledge.md). |
| `/api/v0/admin/*` | Administrative models, grants, settings and catalogs; administration guides. |
| `/api/v0/setup/*` | Verified setup/recovery state machine. |

The running gateway supplies `/openapi.json`, a typed OpenAPI contract for the
registered `/api/v0` routes. It describes operations, path parameters, request
and response schemas, security requirements and the shared error envelope.
Use it as the machine-readable contract for the exact build you are calling.
Most `/api/v0` refusals return an `error` object with a readable `message` and
machine-readable `type` and `code`; validation responses may also include
field-level issues. Check each operation's OpenAPI response schema for details.

### Conversation events

Submitting a message and subscribing to its events are separate operations.
The browser posts JSON to the conversation's messages endpoint and reads the
conversation's `events` endpoint as an SSE stream. Its data frames use the
AIplane JSON event protocol (`snapshot`, `turn_delta`, tool and state events),
not OpenAI chat chunks. Reconnect using the existing protocol's snapshot and
event handling; do not append a reconnect snapshot as if it were new text.

### Personal Dot automation

Schedule and webhook create/update bodies accept `dot_id: string | null`.
Omitting it selects ordinary user execution. A changed binding must select an
active owned Dot; an unchanged unavailable binding can be retained while other
configuration is edited, but cannot execute. Responses include the stored
`dot_id` and the actual owned Dot metadata when that Dot exists.

`linked_session_id` is valid only with conversation reuse. Omission retains
the link, an empty string clears it, and a concrete ID must identify a private
owned conversation with the same Dot binding. Changing the Dot binding clears
the existing link unless a matching replacement is supplied. The chat-session
list accepts `dot_id`: absence lists all conversations, an empty value selects
ordinary conversations, and a Dot ID selects that owner's Dot conversations.
This scope also applies to text search before its result limit.

Run history includes the exact `assistant_turn_id`. A synchronous webhook or
manual rerun that waits for approval returns `202` with status `waiting` and
its conversation ID. Decisions use the existing Inbox and resume the recorded
turn with its original history limit and tool ceiling. Disabling a trigger
stops new firings while allowing a pending decision to settle; deleting a
trigger with a nonterminal run returns `409`. Running background turns can be
watched and stopped through the existing conversation endpoints; interactive
messages, steering and history rewrites are refused while that turn runs.

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
| `HEAD /api/hello` |
| `GET /api/v0/admin/automatic-routes` |
| `PUT /api/v0/admin/automatic-routes` |
| `DELETE /api/v0/admin/automatic-routes/{*alias}` |
| `PUT /api/v0/admin/backends` |
| `POST /api/v0/admin/backends/test` |
| `DELETE /api/v0/admin/backends/{name}` |
| `POST /api/v0/admin/backends/{name}/enabled` |
| `POST /api/v0/admin/backends/{name}/rename` |
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
| `GET /api/v0/admin/upstreams` |
| `GET /api/v0/admin/upstreams/events` |
| `PUT /api/v0/admin/upstreams/fallback` |
| `POST /api/v0/admin/upstreams/reload` |
| `GET /api/v0/admin/users` |
| `POST /api/v0/admin/users/{id}/impersonate` |
| `POST /api/v0/agent-architect` |
| `GET /api/v0/agent-resources` |
| `GET /api/v0/agents` |
| `POST /api/v0/agents` |
| `GET /api/v0/dots` |
| `POST /api/v0/dots` |
| `PUT /api/v0/dots/{id}/active` |
| `GET /api/v0/dots/{id}/access` |
| `PUT /api/v0/dots/{id}/access` |
| `GET /api/v0/dots/{id}/communication` |
| `PUT /api/v0/dots/{id}/communication` |
| `GET /api/v0/dots/{id}/memories` |
| `POST /api/v0/dots/{id}/memories` |
| `PUT /api/v0/dots/{id}/memories/{memory_id}` |
| `DELETE /api/v0/dots/{id}/memories/{memory_id}` |
| `GET /api/v0/dots/{id}/activity` |
| `POST /api/v0/dots/{id}/sessions` |
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
| `GET /api/v0/chat/sessions/{id}/capabilities` |
| `POST /api/v0/chat/sessions/{id}/capabilities` |
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
