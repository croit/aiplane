# Gateway HTTP API

Automatic routing aliases remain wire-compatible with standard clients: send
the alias in the existing `model` field on `/v1/chat/completions`,
`/v1/responses` or `/v1/messages`. The response reports the effective decision in
`X-Gateway-Resolved-Model` and the `X-Gateway-Route-*` headers documented in
[`automatic-routing.md`](automatic-routing.md). No gateway-specific request
field is required.

AIplane exposes OpenAI-, Anthropic-, and TypeSafe System One-compatible APIs. Every `/v1/*` endpoint requires a valid gateway bearer token (see [`auth.md`](auth.md)). The two health probes are unauthenticated.

The routes are wired in `crates/aiplane/src/rama_server/router.rs`; the `/v1/*` handlers live in `crates/aiplane/src/rama_server/proxy.rs`.

## Supported endpoints

| Method | Path | Auth | Notes |
|---|---|---|---|
| POST | `/v1/chat/completions`     | Bearer | Streaming + non-streaming. Server-side tool execution when the caller's token has tool grants (see [`tools-rbac.md`](tools-rbac.md)); otherwise a byte-for-byte passthrough. Routes to the `chat` pool. |
| POST | `/v1/responses` | Bearer or `x-api-key` | OpenAI Responses compatibility, streaming and buffered replies, on the same pipeline as `/v1/chat/completions`; see [Responses API](#responses-api) and [Codex](codex.md). |
| GET | `/v1/responses/{id}` | Bearer or `x-api-key` | A stored response, readable by the person or principal whose token created it. |
| GET | `/v1/responses/{id}/input_items` | Bearer or `x-api-key` | The input items of a stored response, paged with `limit`, `order` and `after`. |
| DELETE | `/v1/responses/{id}` | Bearer or `x-api-key` | Deletes a stored response. |
| POST | `/v1/messages` | Bearer or `x-api-key` | Anthropic Messages compatibility, streaming and buffered replies; see [Claude Code](claude-code.md). |
| POST | `/v1/messages/count_tokens` | Bearer or `x-api-key` | Anthropic-shaped input-token counting for client context management; see [Claude Code](claude-code.md). |
| POST | `/v1/systemone`            | Bearer | TypeSafe System One-compatible typed decisions. Byte-dumb relay to the `system_one` pool; non-streaming. |
| POST | `/v1/embeddings`           | Bearer | Single + batch. Byte-dumb relay to the `embedding` pool; non-streaming. A bare `usage.total_tokens` counts as input tokens. |
| POST | `/v1/rerank`               | Bearer | Byte-dumb relay to the `rerank` pool's `/rerank`; non-streaming. Request and response are relayed unchanged, so the backend's own dialect applies (Cohere, Jina and vLLM take `{model, query, documents, top_n?}`). Recorded as usage kind `rerank`; tokens only when the backend reports `usage` (a bare `total_tokens` counts as input), otherwise no tokens and no cost. |
| POST | `/v1/images/generations`   | Bearer | JSON (`{model, prompt, size, …}`) in, OpenAI images envelope (`data[].b64_json` or `.url`) out. Byte-dumb relay to the `image` pool. |
| POST | `/v1/images/edits`         | Bearer | `multipart/form-data` (`image` + `prompt` + `model`). Byte-dumb relay to the `image` pool. |
| POST | `/v1/audio/transcriptions` | Bearer | `multipart/form-data`, Whisper-compatible. Silence-trimmed and re-framed before forwarding to the `transcription` pool. |
| POST | `/v1/audio/speech`         | Bearer | Text-to-speech (OpenAI-shaped: `{model, input, voice, response_format}`). Byte-dumb relay to the `speech` pool; audio bytes out. Returns a routing error if no `speech` backend serves the model (i.e. no `speech` pool configured). |
| GET  | `/v1/models`               | Bearer | Lists every model served by any healthy backend across public pools (chat, transcription, embedding, rerank, image, speech, system_one), de-duplicated by id. Synthesised from the registry's cached model sets — no upstream round-trip. |
| GET  | `/v1/models/{id}`          | Bearer | Retrieve a single model object, or `404 model_not_found` if no backend serves the id. `{id}` is a catch-all because model ids contain `/`. |
| GET  | `/v1/sandbox/files/{run}/{filename}` | Bearer | Downloads a file a sandbox run produced for the caller, scoped to the caller's user (see `sandbox_api`). |
| GET  | `/healthz`                 | none | Liveness. Returns `{"status":"ok"}`. |
| GET  | `/readyz`                  | none | Returns HTTP 200 and `{"status":"ok"}` after setup completes; before setup, HTTP 503 and `{"status":"setup_required"}`. This checks setup state, not upstream health. |
| GET  | `/metrics`                 | scrape token and/or allowed IPs | Prometheus text exposition (`text/plain; version=0.0.4`). While switched off or switched on without a guard, the answer an unknown path gets (`404`); `403` for a client address outside the allowed IP list, `401` for a missing or wrong `Authorization: Bearer` token (scheme name in any case); with both guards set, both must pass. Configured under `/admin/settings` → Access → Prometheus metrics; see [monitoring](operations/monitoring.md). |

`POST /v1/audio/translations` is **not** implemented — no route is registered.

## System One compatibility

`POST /v1/systemone` preserves TypeSafe's request and response contract. The gateway reads only the `model` field for routing, rewrites it when an alias resolves, and forwards every other field without translation. That keeps `noul`, `choice`, `score`, `instructions`, `criteria`, probabilities, confidence, and future protocol additions under the upstream contract rather than a gateway-owned schema.

The selected backend receives `POST <base_url>/systemone`; System One requests are never translated into chat completions. One provider backend can belong to both the `chat` and `system_one` pools, sharing its identity, base URL, and API key. System One models configured on the `system_one` pool pin that pool's catalog (the shared backend's `/models` is the chat catalog), and pool-scoped model state keeps its catalog and wire protocol isolated from chat. A dedicated System One server needs no list: its `/models`, in OpenAI's or TypeSafe's envelope, is discovered.

Both rolling and pinned provider IDs are ordinary model IDs. For example, an operator can expose `jev-latest` as an alias for `~typesafe/jev-latest` while also offering the version-pinned `typesafe/jev-1.13`. `/v1/models` lists the routable alias and real IDs subject to the caller's pool and token restrictions.

The official TypeSafe JavaScript SDK accepts a custom base URL, so an application can use its normal client against the gateway:

```ts
import { TypeSafeClient, noul } from "@typesafe-ai/sdk";

const client = new TypeSafeClient({
  apiKey: process.env.GATEWAY_TOKEN,
  baseURL: "https://gateway.example.com",
  defaultModel: "jev-latest",
});

const result = await client.systemOne({
  state: { ticket: "My card was charged twice." },
  questions: { urgent: noul("Does this need an immediate response?") },
});
```

The gateway's existing `GET /v1/models` remains OpenAI-shaped. The SDK's `systemOne()` method is compatible; its separate `models.list()` method expects TypeSafe's different model-list envelope and is not currently supported.

> The web UI is a SvelteKit SPA served from `/`; its client routes (`/chat`, `/settings/tokens`, `/admin/*`, …) and the session-scoped `/api/v0/*` and `/auth/*` routes are separate surfaces, not part of the OpenAI-compatible API. See [`ui.md`](ui.md).

## Authentication

Every `/v1/*` call must supply a gateway credential using `Authorization: Bearer <token>` or `x-api-key: <token>`. The bearer header takes precedence. Person tokens use `gwk_`; system-principal tokens use `gws_` and resolve the principal's effective grants. Browser cookies do not authenticate `/v1/*`. AIplane validates active tokens by SHA-256 lookup before doing work. A missing, malformed, or unknown token gets a `401` with an OpenAI-shaped envelope and a `WWW-Authenticate: Bearer realm="gateway"` header:

```json
{
  "error": {
    "message": "missing or invalid bearer token",
    "type": "unauthorized",
    "code": "unauthorized"
  }
}
```

The client's own `Authorization` header is never forwarded upstream — it is dropped and the configured backend key (if any) is injected in its place. Token format and storage details are in [`auth.md`](auth.md).

Model access on proxy paths has two cumulative gates:

- A pool's `allowed_groups` controls which gateway groups can discover and route its models. Admins bypass this operator policy. A model available only through an inaccessible pool is hidden as `404 model_not_found` in both routing and model discovery.
- A bearer token's model allowlist can narrow its owner's access further. Admin status does not bypass a credential's own allowlist. An excluded model is omitted from `/v1/models` and direct routing returns `403 model_not_allowed`.

RBAC also applies to *tools*: together with the user's `/tools` toggles and the token's Off / Auto / On capability settings, it decides which gateway tools can be advertised and executed. Missing built-in settings default to Auto, while MCP connectors and skills default to Off. Auto contributes only `search_gateway_tools` initially; matching tools join a later model round. Off excludes a tool from both disclosure and execution. MCP tools also require an explicit group grant (`mcp__<server>`, a specific tool id, or `*`) and the connector's ACL. A denied tool is simply never offered; it does not itself produce a `403`.

## Model field and alias resolution

The `model` field may be a real model id **or an alias** (see [`upstreams.md`](upstreams.md#model-aliases)). Requests without a string `model` field get `400 invalid_request`.

When an alias — or one of the configured fallbacks — resolves to a different real model, AIplane:

- rewrites the forwarded body's `model` to the real id (upstreams don't know the alias),
- echoes the real id in the response, and
- sets an `X-Gateway-Resolved-Model: <real-id>` response header — **only** when the resolved id differs from what the client sent.

Admin-configured sampling/reasoning defaults are keyed on the *real* model id and only fill in fields the client omitted (client values always win).

## Response headers

Beyond the relayed upstream headers, AIplane may add:

| Header | When | Meaning |
|---|---|---|
| `X-Gateway-Resolved-Model` | Alias/fallback fired | The real model id that actually served the request. |
| `X-Gateway-Tool-Rounds` | Non-streaming chat completion that ran the gateway tool loop | Number of upstream rounds the tool loop took. Absent on the byte-dumb fast path and on streaming responses. |
| `X-Gateway-Tool-Budget-Exhausted` | Non-streaming response whose turn the tool-round budget closed | Always `true` when present. Same fact as the [`aiplane` body field](#tool-round-budget); absent otherwise. |

## Tool-round budget

A request that runs gateway tools (including `web_search_options`) is one whole agentic turn, bounded by a hard round budget (`MAX_TOOL_ROUNDS`, 16 upstream requests). Running out of rounds does **not** discard the work:

1. The last round the budget allows is a *final round*: the model is told its tools are spent and asked to answer from the results it already has. On Ollama, vLLM and SGLang the tool definitions are left out of that request; elsewhere it sends `tool_choice: "none"`. Ollama ignores `tool_choice`. vLLM and SGLang switch their tool parser off under it while the model still sees its tools, so Qwen writes the call out as text instead (see `BackendProfile::honors_tool_choice`).
2. The answer is returned as an ordinary `200` completion, with `finish_reason: "stop"`, plus a top-level signal:

   ```json
   {
     "choices": [{"message": {"role": "assistant", "content": "…"}, "finish_reason": "stop"}],
     "aiplane": {"tool_rounds": 15, "tool_budget_exhausted": true}
   }
   ```

   `tool_rounds` is the number of tool rounds that actually ran. When streaming, the same object rides the chunk that carries the `finish_reason`, the last one before `[DONE]`. The field appears **only** when the budget closed the turn, so a client that ignores it sees a normal completion.
3. A model that calls a tool on the final round anyway never has that call run or handed to the client. That includes a call written out as text (`<tool_call>…` or `<function=…>`): it is cut from the content together with everything after it. When streaming, the final round is therefore held back and arrives in one piece at its end rather than live. If the model also wrote text, that text is the answer. If it wrote nothing, it gets exactly one *closing round* with no tools, where each ignored call is answered as "not run". So a turn takes at most `MAX_TOOL_ROUNDS + 1` upstream requests.
4. Only if the closing round also comes back without text does the request fail: `502` with `code: "tool_budget_exhausted"` (a streamed request ends on an error chunk with the same `code`, then `[DONE]`). The code is never used for a failure that happened before any tool work.

A turn that calls a client-owned tool is handed back to the client as always, and carries no budget signal.

`/v1/messages` and `/v1/responses` follow the same rounds. Their buffered responses set `X-Gateway-Tool-Budget-Exhausted`; neither wire format has a slot for the body field, so a streamed Anthropic or Responses response carries no signal.

## Streaming

`POST /v1/chat/completions` with `"stream": true` returns `text/event-stream`:

- Upstream SSE frames are relayed 1:1 — AIplane does not reframe `data:` lines. The deltas are tapped in parallel through a repetition-based loop guard; a model that collapses into a loop is cut off with a terminating error chunk and `[DONE]`, while a long-but-progressing answer streams through untouched.
- The gateway-owned tool loop opens an upstream stream for each round. It accumulates tool-call deltas, suppresses gateway-owned calls from the client stream, executes them and continues with their results. Client-owned calls are handed back to the client. A budget-closing final round can be held until complete so ignored tool calls cannot leak into its answer.
- This is distinct from the web UI's chat, which posts to `POST /api/v0/chat/sessions/{id}/messages` and reads `GET /api/v0/chat/sessions/{id}/events` — SSE carrying AIplane's own JSON event protocol (`snapshot`, `turn_delta`, `tool_call_done`, …), not OpenAI SSE. See [`ui.md`](ui.md#chat-streaming-the-json-event-protocol).

## Responses API

`POST /v1/responses` serves the OpenAI Responses API — what Codex CLI, the OpenAI Agents SDK and other Responses-only clients speak — on the same pipeline as `/v1/chat/completions`: the request is translated into a chat completion, routed, limited, metered and run through the gateway tool loop exactly like one, and the result is translated back. Setup for Codex is in [`codex.md`](codex.md); the translation lives in `crates/aiplane/src/rama_server/responses/`.

| Request | What AIplane does |
|---|---|
| `instructions`, `system` and `developer` messages | One leading system message |
| `input` (a string or items) | Chat messages: `message` items with `input_text`, `input_image` (URL or data URI) and `output_text` parts; `function_call` / `function_call_output` and `custom_tool_call` / `custom_tool_call_output` items as tool calls and tool results; `reasoning` items are dropped |
| `function` tools | Chat function tools |
| `custom` (freeform) tools | A function taking one string `input`; a grammar, if any, is added to the description. The call comes back as a `custom_tool_call` item |
| Hosted tools (`web_search`, `file_search`, `code_interpreter`, …) | Dropped: they only run on OpenAI's platform |
| `max_output_tokens`, `temperature`, `top_p`, `parallel_tool_calls`, `tool_choice`, `text.format` | `max_tokens`, `temperature`, `top_p`, `parallel_tool_calls`, `tool_choice`, `response_format` |
| `reasoning.effort` | The serving model's reasoning parameter, via the effort level of the same name (`none`/`minimal` → off, `low`, `medium`, `high`, `xhigh`/`max` → xhigh) |
| `input_file` parts | Replaced by a note naming the file: the backends accept text and images only |
| `include`, `prompt_cache_key`, `text.verbosity`, other unknown fields | Dropped, not rejected |
| `background: true`, `conversation`, `prompt`, `item_reference`, an `input_image` by `file_id` | `400 invalid_request_error` naming the `param`: AIplane has no background mode, Conversations API, stored prompts or Files API, and answering without them would answer a different request |

The response is a `response` object with `reasoning` (as `reasoning_text` content), `message`, `function_call` and `custom_tool_call` output items, `output_text`, and `usage` (`input_tokens` and `output_tokens`; a buffered response also carries `cached_tokens` and `reasoning_tokens` where the backend reports them, a streamed one reports them as `0`). `model` is the name the client asked for; the resolved one is in `X-Gateway-Resolved-Model`. A `length` stop is `status: "incomplete"` with `incomplete_details.reason: "max_output_tokens"`.

With `stream: true` the response is the Responses event sequence: `response.created`, `response.in_progress`, per item `response.output_item.added` … `response.output_item.done` with `response.output_text.delta`, `response.reasoning_text.delta`, `response.function_call_arguments.delta` or `response.custom_tool_call_input.delta` in between, and `response.completed` (`response.incomplete` after a `length` stop). Every event carries a `sequence_number`, in the order the client receives the events. While the stream is open AIplane re-sends `response.in_progress` every 15 seconds, so a client watching for events does not abort a turn that is busy without upstream output — a gateway tool running, a slow first token. A failure after the stream started is `response.failed` with `error.code` (`rate_limit_exceeded`, `invalid_request` or `server_error`, or the gateway's own code such as `tool_budget_exhausted`) and the upstream's wording in `error.message`; an item the failure cut off has `status: "incomplete"`. As on `/v1/messages`, a client-owned tool call arrives whole, with a single arguments delta.

Errors before a stream starts use the OpenAI envelope of `/v1/chat/completions`; an upstream error is relayed with its status and body.

### Stored responses

A response is stored when its request has `store` on, which is OpenAI's default. Codex sends `store: false`.

- `previous_response_id` continues a stored response: its input and output items, and those of every response it continued in turn, precede the new `input`. Only the new request's `instructions` apply. A chain whose link is missing — deleted, expired, or created by someone else — is `400 previous_response_not_found` rather than a shorter context. A chain is read up to 1000 responses or 64 MiB deep; past that, start a new one by sending the conversation as `input`.
- `GET /v1/responses/{id}`, `GET /v1/responses/{id}/input_items` and `DELETE /v1/responses/{id}` read, list and delete one.
- A stored response belongs to the person, or the system principal, whose token created it: any of their tokens can read, continue or delete it, nobody else can, and another caller's id answers `404` exactly like an unknown one. Administrators have no view of stored responses. Deleting the person or principal deletes them.
- Stored responses are kept for 30 days. Expired ones are never served and are deleted hourly.
- A streamed response is stored before `response.completed` is sent, so a client may continue it immediately.

## Header handling

Hop-by-hop and identity headers are filtered in both directions. Requests drop `authorization`, `host`, `content-length`, `connection`, `keep-alive`, `proxy-authenticate`, `proxy-authorization`, `te`, `trailer`, `transfer-encoding`, `upgrade`, `expect`, and the client's `accept-encoding` before forwarding. AIplane sends `Accept-Encoding: identity` upstream because it inspects buffered JSON responses for usage accounting and streaming responses for loop protection; forwarding a client's gzip preference would make those bytes opaque while the client silently decompressed them. Responses drop the same hop-by-hop set minus `authorization`/`host`/`expect`. Other end-to-end headers are passed through.

## Schema

We mirror the OpenAI schema for compatibility. We do **not** invent new request/response body fields; gateway-specific signals go in headers (`X-Gateway-Resolved-Model`, `X-Gateway-Tool-Rounds`). The one exception is the [`aiplane` budget signal](#tool-round-budget): a stream's headers leave before the budget is known, so it cannot be a header only. It is additive and appears only when the budget closed the turn. Handlers only read the fields they care about (`model`, `stream`, `messages`, `tools`) and pass the rest through to the upstream unmodified.

## Usage and cost accounting

AIplane records one usage event per upstream call. Token usage accepts both
OpenAI-style `prompt_tokens`/`completion_tokens` and providers that return
`input_tokens`/`output_tokens`. Non-token calls are normalized into billable
units:

- Image generation/editing: generated images and, for edits, one input image.
- Text-to-speech: input characters.
- Transcription: provider-reported duration, or the measured duration of a
  decodable PCM/WAV, MP3, FLAC, Ogg/Vorbis, or ISO-MP4 payload.

Configure prices per model in `/admin/models`. Chat, embedding and rerank
models use prices per 1M tokens. An embedding or rerank call generates
nothing, so when its backend reports only `usage.total_tokens`, those tokens
are recorded as input tokens and priced at the input rate. A chat call
reporting only a total keeps it unsplit. Image, speech, and transcription models use prices per
image, character, or second respectively. Costs are settled when the usage
writer flushes the event and are immutable afterwards. A successful cache hit
on the voice TTS path does not create an upstream event and is therefore free.

Provider-specific cached-token discounts are not currently applied separately;
the reported total input token count is priced at the configured input rate.

## Errors

Errors are returned in the OpenAI envelope so SDKs surface them correctly. The general helper sets `type` and `code` to the **same** value:

```json
{
  "error": {
    "message": "no healthy backend in `chat`",
    "type": "upstream_unreachable",
    "code": "upstream_unreachable"
  }
}
```

An unknown model is the one deliberate exception — it matches OpenAI's `model_not_found` shape exactly, with a distinct `type` and a `param`, so clients treat it as a request error rather than a retryable 5xx:

```json
{
  "error": {
    "message": "The model `foo` does not exist or you do not have access to it.",
    "type": "invalid_request_error",
    "param": "model",
    "code": "model_not_found"
  }
}
```

Upstream 4xx/5xx bodies are relayed verbatim (status, headers, and body), so a provider's own error reaches the client unchanged.

Status codes AIplane itself produces:

| Status | `code` | Cause |
|---|---|---|
| `400` | `invalid_request` | Malformed body, missing `model`, unparseable multipart. |
| `400` | `previous_response_not_found` | `/v1/responses`: a response in the `previous_response_id` chain does not exist, has expired, or belongs to someone else. |
| `401` | `unauthorized` | Missing / malformed / unknown bearer token. |
| `403` | `model_not_allowed` | The credential's model restriction excludes the requested model. |
| `429` | `rate_limit_exceeded` | An applicable request, token or cost limit is exceeded; inspect the response and `Retry-After`. |
| `404` | `model_not_found` | No backend in any pool serves the requested model. |
| `404` | `not_found` | `/v1/responses/{id}`: no stored response with that id belongs to the caller. |
| `500` | `internal_error` | Internal failure or unparseable upstream JSON. |
| `502` | `upstream_unreachable` | A chosen backend was contacted but the transport/read failed. |
| `502` | `tool_budget_exhausted` | Tool rounds ran, but the model never produced text, even in the closing round after the budget ran out. See [Tool-round budget](#tool-round-budget). |
| `503` | `upstream_unreachable` | No healthy backend for the model's pool, or the pool is saturated. |

The shared limit enforcer checks applicable subject, group, global and token rules against the resolved model, after the model is resolved and before a backend slot is taken, so an over-budget caller gets `429` even when the pool is saturated. Every model-routed `/v1` endpoint is gated this way: chat completions, responses, messages, System One, embeddings, rerank, image generation and editing, speech and transcription. On chat completions, responses and messages the check comes before the [content guard](admin/settings.md#access-and-content-guard), so a request refused for its limits costs no guard inference. A pool with `enforce_limits = false` is exempt: its calls do not consume a budget and are not refused once one is spent. An automatic route also checks its selector model right before asking it, so a session-affinity hit is not checked on the selector; see [automatic routing](automatic-routing.md#limits). Configure limits through the administration surfaces and token quotas; see [access and limits](admin/access.md).
