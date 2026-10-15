# Visitors and the public endpoint

A published agent talks to anonymous visitors of a website that embeds it,
through `/api/v0/embed/*` (`aiplane-api::pages::embed`, routed in `gateway`).
This file covers how a visitor's session works, what keeps a public agent
safe to leave running — rates, the owner's budget, retention — and the two
things visitors bring besides messages: identity and voice. What a website
owner pastes and configures is [`embed.md`](embed.md); the credentials are
summarised in [`auth.md`](auth.md#embed-keys-gwe_-and-visitor-tokens-gwv_).

## Sessions

### A script widget with a token in `sessionStorage`

The requirements: the conversation survives a reload, does not span browser
sessions, and expires after a configurable idle TTL (default 30 min, sliding).

| Option | Verdict |
|---|---|
| Gateway cookie in an iframe | Third-party cookies are blocked or partitioned in Safari and Chrome, and a cross-site cookie needs CSRF defences. Rejected |
| CHIPS (`Partitioned` cookie) | Works in Chromium and Firefox, but Safari's support and ITP behaviour are the weak link. It still needs `SameSite=None` and CSRF handling. Rejected |
| **Script in the host page (shadow DOM), visitor token in `sessionStorage`, sent as `Authorization: Bearer`** | Storage is first-party to the host page. `sessionStorage` survives a reload and dies with the tab, which is exactly "reload-safe, not across sessions". No cookie means no CSRF. **Chosen** |

The widget reads events through `fetch` streaming, not `EventSource`, because
`EventSource` cannot send an `Authorization` header.

### Tables

```sql
CREATE TABLE agent_embed_keys (
    id           TEXT PRIMARY KEY NOT NULL,
    principal_id TEXT NOT NULL REFERENCES agents(principal_id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    key_hash     TEXT NOT NULL UNIQUE,      -- SHA-256 of the "gwe_…" key
    origins      TEXT NOT NULL,             -- JSON array, exact scheme://host[:port]
    created_by   TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    revoked_at   TEXT
) STRICT;

CREATE TABLE visitor_sessions (
    id             TEXT PRIMARY KEY NOT NULL,
    principal_id   TEXT NOT NULL REFERENCES agents(principal_id) ON DELETE CASCADE,
    embed_key_id   TEXT NOT NULL REFERENCES agent_embed_keys(id) ON DELETE CASCADE,
    token_hash     TEXT NOT NULL UNIQUE,    -- SHA-256 of the "gwv_…" visitor token
    session_id     TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    client_ip      TEXT,
    idle_ttl_secs  INTEGER NOT NULL CHECK (idle_ttl_secs > 0),  -- so sliding needs no spec parse
    created_at     TEXT NOT NULL,
    last_seen_at   TEXT NOT NULL,
    expires_at     TEXT NOT NULL,           -- slid on every accepted request
    max_expires_at TEXT NOT NULL            -- the absolute cap
) STRICT;
```

- **The embed key is public by design** — it ships in the page source — and
  is still stored only as its SHA-256, like every other credential: storing it
  in clear would make the table a list of working keys. The plaintext is in
  the create response only.
- **Deleting a conversation ends its visitor session** (`ON DELETE CASCADE`);
  `chat_sessions.visitor_id` points back with `ON DELETE SET NULL`.
- Rows live in `aiplane-agents::db::{embed_keys, visitor_sessions}`. Creating
  and revoking a key writes `embed_key_created` / `embed_key_revoked`.
  `visitor_sessions::start` opens the principal-owned conversation
  (`agent_version` = the live version), the visitor row and the back link in
  one transaction. `lookup` resolves a token without touching it; `slide`
  counts an accepted request. Both take `now`.

### Lifecycle

- **Start:** `POST /api/v0/embed/sessions {key}`. The key must hash to a live
  row, the request's `Origin` must be in that key's `origins`, and the agent
  must be enabled and published. The answer is `201 {token, expires_at,
  idle_ttl_secs, agent: {display, color, voice}}` with a fresh `gwv_` token.
- **Every later request** sends `Authorization: Bearer gwv_…` and re-checks
  the chain (`visitor()`): session not expired, key not revoked, `Origin`
  allowed, agent enabled and published. Only then does the session slide, to
  `min(now + idle_ttl, max_expires_at)`. So a revoked key or a disabled agent
  ends open conversations at their next request, and a request that fails
  one of these checks does not slide it. Refusals that come later — a body
  check, the rate and budget gate (`admit`), `turn_in_progress` — happen
  after the slide, so they keep the session alive.
- **Idle TTL:** the starting version's `publish.idle_ttl`, default 30 min.
  **Absolute cap:** 24 h (`MAX_VISITOR_SESSION`); `sessionStorage` normally
  ends the session sooner, with the tab.
- **Reload:** the widget finds its token in `sessionStorage` and calls `GET
  /api/v0/embed/session` for the transcript, or `…/events`. After the TTL either
  answers `401 visitor_session_expired`, and the widget starts a new session
  and resends the message once.
- **The token names the conversation.** No request carries a session id a
  visitor could change (`messages` refuses unknown fields). The token is read
  only by the `/api/v0/embed/*` handlers: every other `/api/v0` route wants a
  session cookie, and `/v1/*` routes by prefix and knows no `gwv_`.

### Routes

| Method | Path | Purpose |
|---|---|---|
| POST | `/api/v0/embed/sessions` | Start a visitor conversation `{key}` |
| GET | `/api/v0/embed/session` | The conversation: `{expires_at, idle_ttl_secs, agent, live_turn_id, turns}` |
| POST | `/api/v0/embed/messages` | `{text}` (≤ 8000 characters, no other field); `202 {turn_id, user_turn_id, placement}` (`queued`, with `turn_id: null`, behind a pending decision) |
| GET | `/api/v0/embed/events` | `chat_json` frames for `fetch` streaming |
| POST | `/api/v0/embed/resume` | `{request_id, decision, value?}`: answer a `secure_input` ([`agent-hil.md`](agent-hil.md#answering)); `202 {turn_id}` |
| POST | `/api/v0/embed/identity` | `{token}`: a host-signed identity ([below](#host-jwt)) |
| POST | `/api/v0/embed/agent` | `{key}` → `{agent: {display, color, voice: {input, output}}}` before any conversation ([Voice](#voice)) |
| POST | `/api/v0/embed/transcribe`, `/api/v0/embed/speak` | [Voice](#voice) |
| GET | `/api/v0/embed/recorder.js` | the audio worklet the widget records with |
| GET | `/api/v0/agents/{id}/embed-keys` | The agent's keys (`read` share); never the key |
| POST | `/api/v0/agents/{id}/embed-keys` | `{name, origins}` (`write` share); `201 {embed_key, key}` |
| POST | `/api/v0/agents/{id}/embed-keys/{key_id}/revoke` | `write` share; `204` |

Errors, all in the `/api/v0` envelope: `401 embed_key_invalid`, `403
embed_key_revoked`, `403 origin_not_allowed` (names the origin to add), `403
agent_disabled`, `409 agent_not_published`, `401 visitor_session_invalid`,
`401 visitor_session_expired`, `409 turn_in_progress`, `429
visitor_rate_limited`, `503 agent_unavailable`, `503
agent_runtime_unavailable`. The envelope's `message` is English with a stable
`code`; the widget shows its own Fluent string (`embed-*`) per code.

### Origins

A request's `Origin` must be in the embed key's `origins` and, when the
conversation's version sets `publish.origins`, in those too; the refusal says
which list lacks it.

**What the origin check is for.** `Origin` can be forged by any non-browser
client and the embed key is public. The origin check stops other websites from
embedding the agent, not abuse. What bounds abuse is the per-visitor and
per-IP rates and the owner's budget ([below](#rates)), the agent's
default-deny grants, and the gates. Nobody should mistake the key for a
secret.

**CORS.** `EmbedCorsLayer` (`aiplane::rama_server::embed_cors`) handles
`/api/v0/embed/*` only; every other `/api/v0` route gets no CORS headers. A
preflight carries neither key nor token, so the layer reflects an `Origin`
only if some live key of an enabled agent lists it, and answers a preflight
from any other origin `403` without CORS headers. The handler then checks the
origin against the request's own key, so an origin the spec excludes still
gets a preflight answer and is refused by the handler. The set of such origins
is cached in memory (`embed_keys::EmbeddableOrigins`): creating or revoking a
key and disabling or deleting an agent clear it at once, and a 30 s TTL catches
writes from outside the process. No `Allow-Credentials`; `Max-Age` 600 s, so a
revoked origin stops working quickly.

### The runner

The endpoint opens the turn rows (the visitor's user turn and an
`in_progress` assistant turn), claims the conversation
(`agents::embed::claim`) and hands an `OpenedTurn` to the installed
`AgentTurnRunner` in a background task (`spawn_guarded`), which calls
`drive_opened` ([`agent-runs.md`](agent-runs.md#entry-points)). A profile that
cannot load (agent disabled, no healthy model) errors the turn with that
reason. Without a runner (only in tests) `messages` answers `503
agent_runtime_unavailable` and stores nothing. A runner that leaves the turn
unfinished, or panics, has its turn errored before the claim drops.

### What a visitor sees

Turns are filtered before they leave: no tool calls, no reasoning, no model
name, no steers, no content of an unfinished answer, and an errored turn
carries a generic message rather than the upstream's. A suspended turn carries
`suspension` as `SuspensionView::for_participant` ([`agent-hil.md`](agent-hil.md#answering)).

### The event stream

Answers are buffered: each is held until the output filter has ruled
([`agent-runs.md`](agent-runs.md#output-filter)), then sent whole.

`GET /api/v0/embed/events` sends `snapshot` first, with `live_turn_id` when a
turn runs. Without one it ends with `idle` (or `suspended` when the
conversation waits). With one it subscribes to the conversation's worker in
the session worker registry and reads that one turn once no worker holds it
(`agents::embed::released`, paced by `pages::turn_wait`). It listens for
`TurnUpdate::Released` only: ticks and `Finalized` come while the answer may
still be unfiltered, so a visitor never sees partial text, tool internals or
an answer the output filter has not ruled on. No polling: the claim is an
RAII guard, so its release always comes. Once the turn is terminal the stream
sends its whole answer as one `turn_delta` with `full: true` and then
`turn_finalized`. While it waits, SSE comment lines (`: working`) every 15 s
keep the connection alive. A stream gives up after 10 minutes with `idle`; the
widget re-attaches. The A2A server streams the same way
([`agent-a2a.md`](agent-a2a.md)).

## Rates

`aiplane_agents::rates`. Limits that make an embedded agent safe to leave
running, read from the agent's **live** version:

- `publish.rate_limits.visitor` / `.ip`: `{max, per}`, both required, `max ≥
  1`, `per` a duration. Defaults when unset: **20 messages per 10 minutes per
  visitor session**, **60 events per 10 minutes per client IP**.

**What is counted is the admission** (`rates::admit_visitor`, `rates::Rate`):
every request the gate lets through is one event, recorded by the one rate
primitive, `rates::record_within`. The windows are read and the event written
in one `WriteTx` (`BEGIN IMMEDIATE`), so parallel requests queue on the write
lock rather than all passing the check before any is counted. It is an exact
sliding window, not the hour-snapped `Window` of spend limits: a visitor told
to wait 40 s may send after 40 s. The per-visitor bucket counts the admissions
into one conversation (`rates::Counter::conversation`, the embed visitor or
the A2A context); the per-IP bucket counts every admission from that IP to the
agent on every channel (`Counter::ip`), so opening a fresh conversation per
message does not dodge the per-visitor limit. The events are rows of
`rate_events`, one per window, each expiring a window after it was written.
A refused request writes nothing, so it never counts; the owner budget is
checked first for the same reason.

Gated: `POST /api/v0/embed/sessions`, `…/messages`, `…/resume`,
`…/transcribe`, `…/speak`, and an A2A message. Reads (`GET …/session`,
`…/events`) cost the agent nothing, and the widget re-attaches freely.

**The client IP** is the one the gateway derives: the TCP peer, or — only
when the peer is in `$AIPLANE_TRUSTED_PROXIES` — the rightmost
`X-Forwarded-For` hop that is not itself a trusted proxy, the same resolution
GeoIP uses. With nothing trusted a forged header changes nothing; behind a
reverse proxy that is *not* listed, every visitor shares the proxy's bucket.

## Owner budget

`limits::Enforcer::check_agent`. The spec's `publish.budget` (`monthly_cost`
> 0 in the currency models are priced in, and/or `monthly_tokens` ≥ 1; no
default) becomes month-window limits labelled `AgentSpec`. An operator may add
a `limits` rule with subject **`system`** (subject id = the agent's id) on
the admin limits page (subject "Agent", the agents listed by name) or at
`/api/v0/admin/limits`. Each is its own ceiling — the
tightest decides, none widens another — measured against
`usage_events.agent_id`.

- **Every model call of a conversation is metered**: the main agent's rounds,
  the sub-agents' (with `agent_id` = the main agent), the router's
  classifier, the topic guard, compaction, the rubric judge and voice. Every
  such usage row has `source = 'agent'` (`UsageSource::Agent`), however the
  conversation came in (embed, A2A, a parent's route, the test chat), so the
  Usage page tells an agent's traffic apart and filters it as *Agents*. Usage
  rows need `usage.enabled`; with metrics off, nothing is ever spent against
  a budget.
- **Debt model**, like every other limit: the message that crosses the line
  is served, the next is refused. The owner's budget is part of the agent and
  applies even with `limits.enabled` off; that switch only governs the
  operator's rules. Both are checked on starting a conversation and on every
  message.
- **Managers see the state** in `GET /api/v0/agents/{id}` under
  `agent.limits`: `{rate_limits: {visitor, ip}, retention_days, budget:
  [{set_by, dimension, window, max, used, exceeded, refreshes_at}], available,
  unavailable_reason}`.

### Refusals

- Rate: `429 visitor_rate_limited` with `Retry-After` (when the oldest counted
  event leaves the window) and the Fluent message `agent-embed-rate-limited`
  in the request's `Accept-Language`.
- Budget: `503 agent_unavailable` with `Retry-After` and
  `agent-embed-unavailable` ("temporarily unavailable"). The visitor is not
  told why.
- Every refusal is a `limit_refused` event on the agent: `{limit:
  visitor_rate|ip_rate, visitor_id, max, per_secs, retry_after_secs}` or
  `{limit: budget, set_by: agent|operator, dimension, window, max, used,
  retry_after_secs}`. The client IP is not stored in it. A flood is folded
  (`embed::RefusalAudit`): one event per (agent, subject, limit) per minute
  with `count: 1` and a `window_id`, and — when more were refused in that
  minute — one more as it closes, whose `count` is the rest and whose `folds`
  names the window. At most 10 000 such windows are open at once; past that, a
  new subject's refusals go to the agent's overflow window for the limit
  (`visitor_id` null), so a storm from rotating IPs is still counted without
  growing memory.

## Retention

`agents::retention`, `db::agent_retention`. A sweeper runs at boot and every
hour (`spawn_retention_sweeper`, started in `main.rs`). Per agent it deletes
the conversations — root sessions it owns, visitor, A2A and test-chat
(`agent_version = 0`) alike — whose last activity (`chat_sessions.updated_at`)
is older than the live version's `publish.retention_days` (default **30**),
together with every sub-agent run below them, however deep. It deletes them
the way a person's chat is deleted, through
`aiplane_features::server::chat_attachments::delete_reclaiming`: the files
their turns reference are listed first, the rows go (one transaction per
conversation, re-checking that it is still idle and waits for nothing), then
the files leave the S3 bucket. The foreign keys take turns, tool calls,
`agent_state`, the visitor session and the A2A context. The selection
requires `user_id IS NULL` at every step, so a person's chat is never
touched. Each sweep that deleted something writes `conversations_swept` with
`{retention_days, conversations, sub_agent_runs}` — counts only. The same
sweep then handles the activity log
([`agent-activity-log.md`](agent-activity-log.md#retention)).

## Identity verifiers

Three verifier kinds under `verifiers.<id>`, the only writers besides the
host of a slot the model cannot write. They are run-scoped synthetic tools
like `set_<slot>` (`aiplane-runtime::agents::verifier`; validation in
`agents/spec/verifiers.rs`; rows in `aiplane-agents::db::agent_verifiers`).

```yaml
verifiers:
  otp:                                   # one-time code through the agent's own connector
    kind: mcp_code
    connector: erp                       # an `agent`-scope connector granted to this agent
    send_tool: send_code                 # default; called with {email}
    check_tool: check_code               # default; called with {email, code}, answers {valid: true, …}
    email_slot: email                    # an `email` slot (the model may write it: it is the claim)
    writes: { verified: result, verified_email: input.email }
    max_attempts: 5                      # default 5, at most 10, per code
    code_ttl: 10m                        # default 10m, at most 1h
    send_limits:                         # sliding windows on sends; defaults shown
      email:   { max: 5,  per: 1h }      # per address, across every conversation
      ip:      { max: 20, per: 1h }      # per client IP
      session: { max: 3,  per: 15m }     # per conversation
    assurance: email_verified            # optional free label, recorded with each outcome
  kyc:                                   # weaker: confirms what the visitor knows
    kind: lookup
    tool: mcp__erp__find_customer        # a granted tool or connector tool; answers {valid: true, …}
    inputs: { name: state.name, number: state.customer_number, region: { const: eu } }
    writes: { verified: result }
    max_attempts: 5                      # per conversation
    assurance: low                       # required
  site:                                  # the embedding website vouches; slots are written as `host`
    kind: host_jwt
    algorithm: HS256                     # or RS256 / ES256
    secret: "…"                          # HS256 only; sealed on save as secret_sealed
    # public_key: "-----BEGIN PUBLIC KEY-----…"   or   jwks_url: https://www.example.com/jwks.json
    issuer: https://www.example.com
    audience: support-agent
    max_lifetime: 10m                    # default 10m: exp - iat may be at most this
    claims: { verified: { customer_id: sub, plan: plan } }   # slot → claim, or field → claim
```

- **Writes.** A source is `result` (the tool's whole answer without
  `valid`), `result.<field>`, or `input.<arg>` — never the code. Every write
  is resolved and checked against its slot first, and stored only if all fit,
  through `write_trusted` with `TrustedWriter::Verifier(<id>)` (or `Host`). A
  target slot must list `verifier:<id>` (or `host`) in `set_by`;
  `set_by: verifier:<host_jwt id>` is refused with "list `host`".
- **The check contract** is fixed: `valid: true` in the tool's answer (a JSON
  object, structured or as text). A RAG search answers hits, not a verdict,
  so a `lookup` names a tool that answers that contract.
- **Validation.** Shape on every save (keys per kind, grants, ranges,
  algorithms, PEM keys parse, JWKS URL, write sources, slot types); what a
  verifier needs to run on publish (`connector`/`email_slot`/`writes`;
  `tool`/`inputs`/`writes`/`assurance`; `algorithm`, a key, `issuer`,
  `audience`, `claims`). One `host_jwt` per spec. An incomplete verifier in a
  draft offers no tool (`Verifiers::from_spec` skips it).

### `mcp_code`

`verify_<id>_request_code()` reads the email slot, checks the three send
windows (`rates::sliding_window`, scopes `email`, `ip` and `session`) and
records the send with the same primitive as a visitor admission
(`rates::record_within` on `agent_verifiers::window` counters, one `WriteTx`,
so parallel requests cannot all pass the check before one is counted), calls
`send_code`, stores the outstanding code's address hash, send time and expiry
(`agent_verifier_codes`), and pauses the turn with
`SuspendRequest::secure_input` (message `agent-verifier-code-sent`, timeout
`code_ttl`). The widget's secure field answers on `/api/v0/embed/resume`; the
same call runs again with `Decided(Value)`, which checks expiry, that the email
slot still hashes to the address the code went to, and takes one attempt in a
single `UPDATE … WHERE attempts < max` (parallel guesses cannot share one),
then calls `check_code`. A miss answers `wrong_code` with `attempts_left`; the
last one answers `locked`. `verify_<id>_submit_code()` asks again for the same
code (`agent-verifier-code-again`) without sending a new one. Both tools refuse
every argument by name (`code`, `provenance`, …).

- **Enumeration.** The visitor and the model get the same pause and the same
  `wrong_code` for an unknown address: `send_code`'s answer (or error) is
  recorded as `delivery: accepted|refused` in the owner's activity log only.
- **Where the code is not.** It reaches the connector's `check_code` and
  nothing else. The verifier's tools declare `sensitive_args`; the layer's
  `get_with_sensitive_args` makes an audited connector record the activity
  log's redaction marker (`{"redacted":true}`,
  `agent_audit::redaction::redacted_arguments`) for both the arguments and a
  failed call's error in `mcp_tool_audit`; the answer is passed through
  `Redaction::withhold` before a slot is written. rmcp dumps outgoing MCP
  requests at `trace`, so the binary's log filter (`aiplane::logging`) pins
  `rmcp::service=debug` whatever `RUST_LOG` asks. `tests/it/embed/verifiers.rs`
  greps every table and every log line (at `trace`, through that filter) for
  the code.

### `lookup`

`verify_<id>()` builds the tool's arguments from state (refusing while a slot
is unset), counts lookups per conversation against `max_attempts` through the
same rate primitive (its `attempts_left` is the count the attempt was admitted
against, `Admitted::seen`), and answers `not_confirmed` alike for no match and
a partial one.

### Host JWT

`POST /api/v0/embed/identity {token}` (visitor token). `host_jwt::accept`
checks that the header's `alg` equals the configured one before anything else
(no HMAC-with-a-public-key confusion), the signature, `exp`/`nbf`/`iss`/`aud`
(30 s leeway; `exp`, `iat`, `iss`, `aud` required), `exp - iat ≤
max_lifetime`, and a `jti`, when present, once per agent (`agent_identity_jtis`,
kept until `exp`).

- **JWKS.** Documents are cached five minutes per URL and refetched for an
  unknown `kid`, at most once a minute — the gateway's one JWKS cache,
  `aiplane_core::server::auth::jwks`, which the OIDC login uses too. The JWKS
  URL is chosen by the agent's owner, so it is fetched through the same guard
  as an A2A route (`outbound_guard`, `Policy::agent`,
  [`agent-a2a.md`](agent-a2a.md#ssrf)): resolved and pinned, no redirects, at
  most 64 KiB, link-local always refused, and loopback, private addresses and
  plain `http` only under `$AIPLANE_ALLOW_PRIVATE_NETWORKS=true`. Saving a spec
  checks `jwks_url` against the same policy (`host_jwt::check_jwks_url`,
  through `outbound_guard::check_url`), so a URL the run would refuse is
  refused on save with the same reason; only the DNS answer is left to run
  time.
- **Answers:** `200 {slots}`, `401 identity_token_invalid` (the message says
  what is wrong, never a claim value), `409 identity_token_replayed`, `422
  identity_not_configured`, `503 identity_keys_unavailable`. The `503`
  message is generic — no URL, status code or parse error, so the endpoint is
  no probe into the gateway's network; the reason goes to the log and to the
  `host_identity` event (`jwks_url`, `error`), where the owner sees it.
- **Storing.** A refused token writes nothing. An accepted one is stored in one
  transaction: every mapped slot is checked first, then the `jti` is spent and
  all slots are written together (`state::write_trusted_all`), so a failed
  write leaves no slot behind and the `jti` unspent — the website can retry
  with the same token.
- **Secret.** The plain HS256 `secret` (at least 32 characters) is sealed on
  save ([`agent-spec.md`](agent-spec.md#validation)).

### Audit

`verifier_outcome` (`{verifier, kind, step, outcome, assurance, session_id,
turn_id, …}`; outcomes `code_sent`, `verified`, `wrong_code`, `locked`,
`expired`, `email_changed`, `rate_limited`, `not_confirmed`,
`too_many_attempts`, `write_failed`) and `host_identity` (`{session_id,
outcome, reason | slots}`, in the conversation's chain). A refused token is
counted like a refused visitor (`embed::RefusalAudit`): one event per
conversation and reason when a window opens, one with the rest of the `count`
when it closes. Never a code, an address or a claim value.

The widget renders a `secure_input` pause as a masked field and sends the host
token once per conversation ([`embed.md`](embed.md#signed-in-visitors)).

## Voice

A visitor may speak a message and hear answers. `publish.voice: { input,
output, voice?, transcription_model?, speech_model? }`
(`spec::model::VoiceSpec`); both directions are off by default.

**Models.** A model named here must be granted to the agent. A direction that
is on and names none runs on the gateway's default transcription or speech
model ([`agents.md`](agents.md#models)), which must then be granted; publishing
a direction without either is refused (`422` at
`publish.voice.transcription_model` / `speech_model`, naming the setup's
*Website* step). Either way an agent reaches only models granted to it, and the
endpoints answer `503 voice_unavailable` when the grant is gone. `voice` is the
TTS voice; unset, the voice the pool of the backend that synthesises it maps
the visitor's language to (`Acquired::voice_for`) applies. Set, it must be one
of the voices the speech model offers (`UpstreamRegistry::speech_voices_of`:
the serving speech pools' `offer_voices`, then their language map's voices —
the list `speech_voices_for` gives the chat's voice picker); publishing
anything else is refused at `publish.voice.voice`, naming the voices on offer.
A draft may hold it. `GET /api/v0/agent-resources` lists each speech model's
`voices`.

**Endpoints** (`aiplane-api::pages::embed::voice`, visitor token and origin
chain as for messages):

| Method | Path | |
|---|---|---|
| POST | `/api/v0/embed/agent` | `{key}` → `{agent: {display, color, voice: {input, output}}}` before any conversation; not rate-gated (reads cost nothing). `start` and `session` return the same `agent` |
| POST | `/api/v0/embed/transcribe` | body `audio/wav`, 16 kHz mono 16-bit PCM, at most **2 MiB** (`413 payload_too_large`), 0.4–60 s (`400 audio_too_short`, `413 audio_too_long`), anything else `415 unsupported_audio` → `{text}` (at most 8 000 characters). Not posted to the conversation: the widget puts it in the input for the visitor to read, change and send |
| POST | `/api/v0/embed/speak` | `{turn_id}` → `audio/mpeg` (`204` when nothing is speakable). Only a `completed` assistant turn of *this* visitor's conversation that no worker holds any more (`404 turn_not_found`, `409 turn_not_final`): the content as stored after the output filter ruled, never text from the client; Markdown is turned into speakable prose (`speech::to_spoken`), and at most 3 000 characters (to the last whole sentence) are spoken |
| GET | `/api/v0/embed/recorder.js` | the SPA's `web/static/pcm-recorder.js` (`include_str!`, one file), served under the embed CORS a cross-origin worklet needs |

A direction that is off answers `404 voice_not_enabled`; a failing backend
`503 voice_unavailable` (the real error is in the activity log and the server
log). Both calls go through `admit` first, so they count against the visitor's
and the IP's rate like a message and are refused once the owner's budget is
spent. Bodies are read with `read_body_capped` / `read_json_capped` under
`BodyLimitLayer::HANDLER_CAPPED`, and a recording is trimmed by the VAD
(`aiplane_features::server::vad`). A spoken turn is cached in memory per turn,
model and voice (256 entries, 64 MiB at most), so replaying costs no second
synthesis; a replay still counts against the rate.

**Usage and log.** Each call that reaches a backend is a usage row of the
agent run (`kind` `transcription` with the recording's seconds, or `speech`
with the characters spoken; `agent_id` set, so it spends the owner's budget)
and an `llm_exchange` in the conversation's chain with `purpose:
transcription` or `speech` (`RunLog::visitor`, an event of a visitor's
conversation between turns), the visitor on the event and, for speech, the
`turn_id`. The transcription event keeps the request without the audio
(`file: {content_type, bytes, seconds}`) and the transcript; the speech event
keeps the text sent and `{content_type, bytes}` of the audio. The chain is
anchored after each call. **Audio is never stored**: a recording lives in
memory for the request, and spoken audio only in the bounded cache.

## The widget

`web/embed/`, a standalone bundle (own Vite config, no SvelteKit), built by
`build-web` to `target/frontend/build/embed.js` and served at `/embed.js`
from `AIPLANE_STATIC_DIR`. It must not pull in the SPA.

- **Shadow DOM, daisyUI inside it.** Tailwind v4 + daisyUI v5 are compiled for
  the widget only and adopted as a constructed stylesheet (not subject to the
  host's `style-src`). Theming is daisyUI's custom properties on the
  `croit-aiplane-embed` element plus `data-theme`; `profile.color` paints it.
- **Session lifecycle** is `TokenStore` + `EmbedApi` (`web/embed/api.ts`): a
  token in `sessionStorage` (accessor and calls guarded, memory fallback),
  resume on load, `visitor_session_expired` starts a fresh session and resends
  the message once, `Authorization` header, `credentials: 'omit'`.
- **Safe rendering.** Answers are parsed by a small markdown subset
  (`markdown.ts`) into a tree and built with `createElement`/`textContent`;
  there is no `innerHTML`. The SPA's `marked` + DOMPurify are left out to keep
  the bundle small.
- **Voice** (`web/embed/voice.ts`, `audio.ts`, `theme.ts`; recording is
  `web/shared/voice-recorder.ts`, the SPA's recorder too, and
  `web/shared/wav.ts` the shared WAV encoder). A microphone button when
  `voice.input`: held down it records until release, a short click starts a
  recording the next click (or Enter/Space) sends; a recording stops by itself
  at 60 s; Escape or *Cancel* throws it away; a refused permission, a missing
  microphone or an insecure page each get their own message. A speaker toggle
  when `voice.output`: off until the visitor switches it on (that click
  unlocks audio, so nothing ever autoplays), then every answer that finishes
  is read aloud; *Stop* ends it. Playback decodes into Web Audio, so no
  `blob:` URL is needed. Animations stop under `prefers-reduced-motion`. The
  recorder logic is a pure state machine (`micStep`) with unit tests.
- **Strings** are the `embed-*` Fluent keys. `gen-locales` also writes
  `web/embed/locales.generated.ts`, checked by
  `i18n_drift::the_embed_catalog_matches_the_fluent_sources`.
- **`mise run dev-ui`** installs `LiveAgentRunner` and seeds a published
  agent on `demo-model` with a fixed embed key for `http://localhost:8000`
  ([`embed.md`](embed.md#try-it-locally)).

Tests: `tests/it/embed.rs` and `tests/it/embed/` (`limits.rs` for rates,
budget and retention; `suspend.rs`, `hil.rs`, `verifiers.rs`, `voice.rs`),
`agents/embed.rs`, `aiplane-agents`' `rates.rs`,
`web/embed/{voice,theme,api}.test.ts`,
`web/shared/{wav,color,voice-recorder}.test.ts`.
