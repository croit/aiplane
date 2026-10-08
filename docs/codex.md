# Codex CLI through AIplane

Codex CLI speaks the **OpenAI Responses API** and nothing else: it accepts only
`wire_api = "responses"` for a custom provider. AIplane serves that API at
`POST /v1/responses`, translating to and from the chat-completions dialect every
configured upstream already speaks. Point Codex at AIplane and it runs against
whatever model you serve, with AIplane's routing, per-token model allowlists,
rate limits, quotas, usage accounting and audit trail applying exactly as they
do to every other `/v1` caller.

Nothing about Codex is patched or wrapped: it is configured through its own
client configuration, `~/.codex/config.toml` on the developer's machine. On the
AIplane side nothing new is configured — the token, aliases and pools are the
ones set in the web UI.

## Set it up

1. **Mint a token.** Sign in to AIplane, open `/settings/tokens`, create one.
   It looks like `gwk_…`. Keep it in your shell environment, not in a file:

   ```bash
   export AIPLANE_TOKEN=gwk_…
   ```

2. **Pick the model name Codex sends.** Codex sends whatever `model` names. Use
   a model id or alias AIplane serves — `/admin/upstreams` lists both. An alias
   such as `default` keeps the client configuration stable while the operator
   moves it between backends.

3. **Add AIplane as a provider** in `~/.codex/config.toml`:

   ```toml
   model = "default"                 # a model id or alias from /admin/upstreams
   model_provider = "aiplane"
   model_context_window = 262144     # what your model actually serves

   [model_providers.aiplane]
   name = "AIplane"
   base_url = "https://aiplane.example.com/v1"
   env_key = "AIPLANE_TOKEN"         # Codex sends it as `Authorization: Bearer`
   wire_api = "responses"
   ```

   `model_context_window` matters because Codex knows nothing about a model it
   has no metadata for: it prints `Model metadata for 'default' not found.
   Defaulting to fallback metadata` and plans against a generic window. Set it
   to the backend's `max_model_len` (shown per backend on `/admin/upstreams`).
   The warning itself stays, and is harmless.

4. **Check it.**

   ```bash
   curl -s https://aiplane.example.com/v1/responses \
     -H "Authorization: Bearer $AIPLANE_TOKEN" \
     -H 'content-type: application/json' \
     -d '{"model":"default","input":"reply with OK"}'
   ```

   Then run `codex`, or `codex exec "…"` for a one-shot task.

To keep your OpenAI setup next to it, put `model`, `model_provider` and
`model_context_window` in a `[profiles.aiplane]` table instead and start Codex
with `codex --profile aiplane`.

## What Codex runs, and where

Codex brings its own tools — the shell, `apply_patch`, `update_plan`, its MCP
servers — and runs them on your machine. AIplane hands every call to one of
them back to Codex as a `function_call` (or, for a freeform tool, a
`custom_tool_call`) item, exactly as the model produced it.

AIplane's own server-side tools — web search, RAG, the sandbox, your connected
MCP integrations — join the turn only when **tool use** is turned on for the
token on `/settings/tokens`. They then run gateway-side and invisibly, between
Codex's own calls, in the same tool loop `/v1/chat/completions` uses. With tool
use off, `/v1/responses` is pure format translation.

Hosted tools that only exist on OpenAI's platform (`web_search`, `file_search`,
`code_interpreter`, `image_generation`, `computer_use`) are dropped from the
request: the model never sees them. Use AIplane's own tools for those jobs.

## Reasoning

Codex's `model_reasoning_effort` setting (sent as `reasoning.effort`) is mapped
onto AIplane's effort level of the same name and from there onto the serving
model's own reasoning parameter, per `/admin/models`:

| `reasoning.effort` | AIplane level |
|---|---|
| `none`, `minimal` | off |
| `low` | low |
| `medium` (and unknown values) | medium |
| `high` | high |
| `xhigh`, `max` | xhigh |

The model's reasoning streams back as a `reasoning` item with `reasoning_text`
content. It is the model's own reasoning, not a summary, so the item's
`summary` is empty; Codex shows it when `show_raw_agent_reasoning = true`.
AIplane has no `reasoning.encrypted_content` to give, and reasoning items a
client sends back are dropped before the backend sees them.

## Stored responses

Codex sends `store: false` and the whole conversation with every request, so
nothing it does is stored. Other Responses clients that rely on
`previous_response_id` are covered in
[`gateway-api.md`](gateway-api.md#responses-api).

## Limits, cost and audit

Identical to `/v1/chat/completions`, because it is the same pipeline:

- every upstream round is one usage row (`/usage`, and `/admin/tokens` for the
  deployment-wide view), attributed to the token and its owner;
- rate limits and quotas are enforced for the resolved model before anything
  is forwarded, and a breach is a `429` with `Retry-After`; a pool exempt from
  enforcement is never refused; an automatic route's selector is checked the
  same way right before it is asked, so a session-affinity hit is not
  checked on it
  ([automatic routing](automatic-routing.md#limits));
- a token's model allowlist applies to the alias *and* its target;
- pool group restrictions apply.

## Streams and timeouts

Codex aborts a stream that delivers no event for `stream_idle_timeout_ms`
(five minutes by default). AIplane re-sends `response.in_progress` every 15
seconds while the stream is open, so a turn that is busy without upstream
output — a gateway tool running, a slow first token — is never mistaken for a
dead one.

While a request is **parked** waiting for a replica to come back
([`upstreams.md`](upstreams.md#waiting-out-an-outage)) nothing has been sent
yet, not even the response headers. AIplane parks a request for at most two
minutes and then answers `503` with `Retry-After`.

A failure after the stream started arrives as `response.failed` with an error
`code`: `rate_limit_exceeded` for an upstream `429`, `invalid_request` for
another `4xx`, `server_error` otherwise, and the upstream's own wording in
`message`.

## Known limits

- **No hosted tools**, see above.
- **Cached and reasoning token counts are `0` on a stream.** Codex streams, so
  its usage display shows input and output tokens only. Prefix caching still
  happens upstream.
- **The repetition guard applies**, as on every streamed `/v1` turn: a model
  that collapses into a loop is cut off.

## Related

- [`docs/gateway-api.md`](gateway-api.md#responses-api) — the endpoint itself
- [`docs/claude-code.md`](claude-code.md) — the same for Claude Code
- [`docs/upstreams.md`](upstreams.md) — pools, backends, aliases, fallbacks
- [`docs/tools-rbac.md`](tools-rbac.md) — who may use which gateway tool
