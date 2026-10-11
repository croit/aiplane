# Agents over A2A

AIplane speaks the Linux Foundation's *Agent2Agent (A2A) Protocol
Specification v1.0.0* (`a2a-protocol.org`, `specification/a2a.proto`, package
`lf.a2a.v1`), JSON-RPC binding (§9), in both directions: a published agent can
be **served** to other agent platforms, and a route can **call** another
platform's agent. The 0.3 wire (`message/send`, lowercase states, `kind`
fields, the `url` / `preferredTransport` card) is not spoken either way.

## Serving an agent over A2A

Handlers in `aiplane-api::pages::a2a` (routed in `gateway`); the spec section,
the card and the state mapping in `aiplane-runtime::agents::a2a` and
`agents/spec/a2a.rs`.

- **Opt-in.** `publish.a2a: { enabled: true, skills?: [{id, name,
  description, tags?, examples?}] }`, validated on save: `enabled` is a
  required boolean, at most 20 skills, ids are slugs and unique. It is read
  from the agent's **live** version, like the limits: publishing a version
  without it stops serving at once. Without `skills` the card derives them:
  one for the agent as a whole (its principal's description, tagged with the
  route names) and one per route that has a `description`; a route without one
  is internal and not advertised.
- **Agent card** — `GET /a2a/agents/{id}/agent-card.json`, public, `404`
  unless the agent is enabled, published and opted in. `name` is
  `profile.display` (else the principal's display), `version` the live
  version, `supportedInterfaces` one `JSONRPC` interface at
  `{public_url}/a2a/agents/{id}` with `protocolVersion: "1.0"`,
  `capabilities: {streaming: true, pushNotifications: false,
  extendedAgentCard: false}`, `defaultInputModes`/`defaultOutputModes`
  `text/plain`, `securitySchemes: {aiplaneSystemToken: {httpAuthSecurityScheme:
  {scheme: "Bearer", bearerFormat: "gws_"}}}` with a matching
  `securityRequirements` entry. `Cache-Control: max-age=300` and an `ETag` of
  id and version. One gateway serves many agents, so there is no
  `/.well-known/agent-card.json`: the per-agent card URL is the one to
  configure in a client or a registry. The card is unsigned (card signing per
  §8.4 would need a gateway signing key, its publication as a JWKS and
  RFC 8785 canonicalisation) and served over TLS.
- **Callers.** A caller is a system principal with a `gws_` token whose
  principal holds grant kind **`a2a_caller`** with the agent's id as `ref`.
  Default deny: a token without that grant, or with one for another agent,
  gets `403 PERMISSION_DENIED`; no token or a bad one `401` with
  `WWW-Authenticate: Bearer`; a person's `gwk_` token `403`. Granting
  `a2a_caller` follows the grant-time cap: the manager needs `write` on that
  agent by the one access rule (`effective_access`), since letting another
  platform call the agent changes the agent. The grant transaction checks
  current access to the referenced agent: no access returns `404`, and Read
  access returns `403` without adding a grant. A token a manager minted loses the
  grant whenever the manager does not hold that. The caller gets nothing of its
  own into the run: the task runs as the agent's principal, with its grants
  ([`auth.md`](auth.md#a2a-callers-gws_--a2a_caller)).
- **Version.** Every request carries `A2A-Version: 1.0` (header, or the
  `A2A-Version` query parameter). A missing header means 0.3 per §3.6.2, so it
  is refused like any other version with `-32009 VersionNotSupportedError`;
  `message/send` answers `-32601`.

**Mapping.**

| A2A | AIplane |
|---|---|
| context (`contextId`) | a root conversation owned by the agent's principal (`chat_sessions.id`), with an `a2a_contexts` row naming the caller, its token and client IP. Only that caller finds it; anyone else gets "not found" (§13.1). Pinned to the version live when it was opened, like a visitor's |
| task (`id`) | one assistant turn of that conversation, and the user turn before it |
| `SendMessage` without `taskId` | a new turn: in a new context, or in the caller's `contextId` |
| `SendMessage` with `taskId` of an `INPUT_REQUIRED` task | the answer to a `secure_input` pause, through the same `agents::resume::claim` / runner `resume` as `POST /api/v0/embed/resume` (`ResumedBy::Participant`), so a verifier's code goes to the tool and nowhere else (not the transcript, the task, the model) |
| `TASK_STATE_WORKING` | turn `in_progress`, or terminal while the runner still holds it (the output filter has not ruled). Per task: the claim names the turn holding the context (`SessionWorkers::holds`), so a finished task reads as finished — and `CancelTask` on it is `-32002` — while a later task of the same context runs |
| `TASK_STATE_COMPLETED` | `completed`; the answer is the artifact `answer` and the last `history` message |
| `TASK_STATE_FAILED` | `errored`; `status.message` is the generic `embed-error-generic` text, never the upstream's |
| `TASK_STATE_CANCELED` | `cancelled` |
| `TASK_STATE_INPUT_REQUIRED` | `suspended`, any kind. `metadata.aiplane` says `{kind, answeredBy: caller \| staff, requestId, expiresAt}`; `status.message` is the tool's message, or `agent-embed-decision-for-staff` for an approval or a hand-off |
| `CancelTask` | running: the conversation's stop flag ([`agent-runs.md`](agent-runs.md#stopping-a-turn)), then the turn ends `cancelled`; paused: every level of the pause is cancelled (`cancel_suspended_turn`); terminal: `-32002`. A running task's cancel waits up to 15 s for the run to notice the flag (between rounds or upstream chunks) and returns the task as it is then |
| `GetTask` / `historyLength` | the task built from the turns; `0` omits `history`, `n` keeps the last `n` |

Blocking is the default (§3.2.2): `SendMessage` returns once the task is
terminal or `INPUT_REQUIRED`; `returnImmediately: true` returns the `WORKING`
task at once. The run is spawned either way, so a caller that hangs up does
not stop its task.

**Streaming over buffered answers.** `SendStreamingMessage` and
`SubscribeToTask` answer `text/event-stream`, one JSON-RPC response per
`data:` frame: the `task` first, then — once the task has stopped working —
its whole answer as one `artifactUpdate` (`lastChunk: true`) and a
`statusUpdate` with the final state; a paused, failed or cancelled task gets
the `statusUpdate` alone. `: working` comment lines every 15 s; the stream
closes after 10 minutes, and the caller re-subscribes or polls.

**Exactly like an embed visitor** — one path, not a fork:
- the turn is opened like `/api/v0/embed/messages` and run by the installed
  `AgentTurnRunner`, so grants, gates, binds, budgets, suspend and resume, the
  output filter and the inbox apply as they do there;
- **admission** is `agents::embed::admit` with `Admission { a2a_context, ip
  }`: the live spec's `publish.rate_limits.visitor` counts the admitted
  messages of one context — the one that opened it too, counted once the
  context has an id (`embed::Admitted::opened`) — and `…ip` counts every
  admission from a client IP, embed ones included; the owner budget and the
  operator's `system` limits apply ([`agent-visitors.md`](agent-visitors.md#rates)).
  A refusal is a JSON-RPC error `-32000` with `RATE_LIMITED` or
  `AGENT_UNAVAILABLE`, a `Retry-After` header and the Fluent message, audited
  as `limit_refused`; nothing is stored and nothing runs;
- **retention** sweeps the conversation, and `a2a_contexts` goes with it.

A message into a context that waits on a task is refused (`CONTEXT_WAITING`)
rather than queued as the widget's is: an A2A message without a `taskId` is a
new task, and that task would have no id until the pause is settled.

**The caller in the call chain.** `RunChain.caller` is `RemoteCaller
{protocol: "a2a", principal_id, name, token_id}` (serialized only when set),
carried into sub-agent chains. `OpenedTurn.caller` sets it, and
`agents::resume` rebuilds it from `a2a_contexts`, so a resumed task names its
caller too. Every `tool_call`, `run_suspended`, `run_resumed`,
`output_blocked`, usage and `mcp_tool_audit` row of the task therefore names
the caller. Each started, answered or cancelled task is also an `a2a_task`
event on the agent: `{action: message | input | cancel, context_id, task_id,
caller_id, caller_name, token_id}`, never the text.

**Errors.** JSON-RPC 2.0 envelopes (§9.5): `error.data` is one
`google.rpc.ErrorInfo` whose `reason` names the error. A2A's codes where they
apply (`-32001` task not found, `-32002` not cancelable, `-32003` push
notifications, `-32004` unsupported — `ListTasks`, `GetExtendedAgentCard`, a
message to a terminal task —, `-32005` a non-text part or output mode, `-32009`
version); the standard ones for parse, envelope, method and params; and
`-32000` (implementation-defined, unassigned by A2A) for AIplane's own:
`UNAUTHENTICATED`, `PERMISSION_DENIED`, `AGENT_NOT_SERVED`, `RATE_LIMITED`,
`AGENT_UNAVAILABLE`, `TASK_IN_PROGRESS`, `CONTEXT_WAITING`,
`DECISION_FOR_STAFF` (the caller tried to answer an approval or hand-off),
`NOT_WAITING`. Authentication errors are HTTP 401/403, an unserved agent 404;
every other error is HTTP 200, as JSON-RPC over HTTP expects.

**What the server does not speak:** parts other than text, in or out
(`ContentTypeNotSupportedError`); push notifications, `ListTasks` and the
extended card (the card says so in `capabilities`). The client's `messageId`
is not stored; `history` messages carry the turn ids. `referenceTaskIds`,
`extensions` and `metadata` are accepted and ignored.

Tests: `crates/aiplane/tests/it/a2a.rs` on wiremock upstreams (the card only
for an opted-in, published agent; refused tokens with nothing run; the
`a2a_caller` grant cap; version and envelope errors; a completed task, its
context and audit; a second task in the same context; `GetTask` and
`historyLength`, refused to another caller; cancelling running and paused
tasks; the output filter with the caller in the chain; rate and budget
refusals; a secure input answered on the task; an approval the caller cannot
give; the streamed task). Unit tests in `agents/a2a.rs`, `agents/spec/a2a.rs`,
`pages/a2a/`, `aiplane-agents`' `db/a2a_contexts.rs`, `run_chain.rs`,
`agents/embed.rs`.

## External agents as route targets

A route may hand the visitor's request to another platform's agent that speaks
A2A v1.0. Code in `aiplane-runtime::agents::a2a_client` (`guard`, `card`, the
exchange) and `agents/spec/route_kinds.rs` (validation); the waiting task in
`aiplane-agents::db::agent_a2a_tasks`.

```yaml
routes:
  partner:
    when: { all: [ { slot: issue, set: true }, { slot: verified, provenance: host } ] }
    task: "Warranty question: {issue}"
    bind: { customer: state.verified.customer_id }   # sent as one `data` part
    a2a:
      card_url: https://partner.example.com/.well-known/agent-card.json
      auth: { kind: bearer, token: "…" }              # sealed on save as token_sealed
      # or { kind: api_key, scheme?: <card scheme>, token }
      # or { kind: oauth_client_credentials, client_id, client_secret, scopes? }
      finish: { schema: { type: object, required: [answer], properties: { answer: { type: string } } } }
      budget: { seconds: 120 }                        # default 120, at most 900
```

- **What leaves the gateway.** `SendMessage` with a text part (the rendered
  `task`) and, when the route binds anything, one `data` part with the bound
  values (`{"customer": "K-12345"}`). Never the transcript, the slots or a word
  the model wrote. Headers: `A2A-Version: 1.0`, the credential, nothing else.
  `configuration.returnImmediately` is `false`; a task the peer still reports
  as submitted or working is polled with `GetTask` every 500 ms. There is no
  streaming, push or file part on this side.
- **The card** (`card.rs`) is fetched from `card_url` and cached for five
  minutes per URL. It must have `name`, `capabilities` and a
  `supportedInterfaces` entry with `protocolBinding: JSONRPC` and a `1.x`
  `protocolVersion`; the first such entry is the endpoint, and its `tenant` is
  sent along. `securitySchemes` and `securityRequirements` decide the auth.
  Skills, modes and signatures are not checked; a signed card is not verified.
- **Auth, limited to what a principal can hold.** `auth.kind` picks one of the
  card's schemes (by `scheme` name, else the first of its type): `bearer` →
  `httpAuthSecurityScheme` with `scheme: Bearer`; `api_key` →
  `apiKeySecurityScheme` in a header (a query or cookie key is refused, so a
  secret never sits in a URL); `oauth_client_credentials` →
  `oauth2SecurityScheme.flows.clientCredentials.tokenUrl`, a
  `client_credentials` form post, the token cached until 30 s before it
  expires. The cache key is the token URL, the client id, the SHA-256 of the
  secret, the sorted scopes and the agent's principal id, so a route with
  another secret (a wrong one included), other scopes or of another agent signs
  in itself and never rides on a token it did not earn. `token` and
  `client_secret` are sealed on every save (`token_sealed` /
  `client_secret_sealed`, [`agent-spec.md`](agent-spec.md#validation)). A card
  that requires auth when the route brings none, or offers no scheme of the
  route's kind, ends the route `incomplete` saying which. OpenID Connect, mTLS
  and authorization-code flows are not offered: each needs a person or a
  client certificate the principal does not have.
- **The grant.** *Chosen:* a grant kind of its own, `a2a_agent`, whose `ref`
  is the exact card URL. Reusing connector grants would mean an `mcp_catalog`
  row the MCP manager would try to connect to. The validator requires the grant
  (`routes.<r>.a2a.card_url`, naming the `POST …/grants {"kind": "a2a_agent",
  "ref": …}` to make), and the dispatch checks it again, so a revoked grant
  sends nothing (`forwarded: false, reason: not_granted`). **Grant-time cap:**
  an external agent is nothing a manager holds, and the grant lets
  visitor-derived data leave the gateway, so only an admin may make it (`403
  grant_exceeds_manager` otherwise); the ref must pass `check_card_url`:
  `outbound_guard::check_url` under the same `Policy::agent` the dispatch uses,
  no credentials or fragment in it. A spec naming the card is checked the same
  way (`SpecContext::allow_private`), so granting, saving and running agree.
  The card URL stays on the guard even though an admin grants it: the endpoint
  and token URLs come from the card, which the remote side writes.
- **One origin.** The endpoint and the OAuth token URL the card names must
  share the granted card URL's origin (scheme, host, port), or the route ends
  `incomplete` before anything is sent. The grant names the card URL, so a card
  that points elsewhere — a compromised CDN, a stale host taken over — must not
  collect the route's credential or the task's bound values.
- **Size caps** (`capped_read::read_capped_for`). Every body read from outside
  — the card (256 KiB), the JSON-RPC answer (1 MiB), the OAuth token answer
  (64 KiB) — is refused when its `Content-Length` is over the cap, before a
  byte is read, and otherwise read chunk by chunk and dropped the moment the
  running total passes it. A chunked body without a length therefore cannot
  make the gateway buffer more than the cap.
- **The result** is the first `data` object among the completed task's
  artifact parts (then its status message), else the first text part that
  parses as a JSON object (a fenced block too); a direct `message` answer is
  read the same way. It must pass the route's `a2a.finish.schema`, else the
  outcome is `incomplete` with reason `failed` naming the violations. So is no
  structured result, a task that ends `FAILED`/`REJECTED`/`CANCELED` (with the
  peer's status text, cut to 300 characters), `AUTH_REQUIRED`, a JSON-RPC
  error, a non-2xx answer, an answer over 1 MiB, or a guard refusal. Running
  past `budget.seconds` is `seconds_exhausted`. The tool result is
  `{forwarded: true, route, remote_agent, outcome, note}`; the main agent's
  `Flag` policy screens it like a sub-agent's. A remote answer is never trusted
  text for the output filter.
- **`input-required`.** *Chosen:* the run pauses only when the peer's status
  message carries a `data` part — a structured request for input — and the run
  can pause. The call then suspends as `secure_input` with the catalog text
  `agent-a2a-input-required` (never the peer's own words, which a visitor would
  otherwise read as ours) and a 10-minute deadline, and records the remote task
  and context ids in `agent_a2a_tasks` keyed by the waiting turn and call. The
  visitor's value goes back as `{"data": {"value": …}}` on that
  `taskId`/`contextId` when `forward_request` runs again with `Decided(Value)`
  (it takes the row, so a task is continued once; without a row the value is a
  staff answer to a hand-off), and the driver's secure-input scrub keeps the
  value out of everything stored. Free-text `input-required` is `incomplete`:
  a route cannot hold a conversation with the peer, and the model must not
  answer it on the visitor's behalf.
- **Audit and debug.** `sub_agent_dispatched` and `sub_agent_finished` with
  `{route, target: "a2a", card_url, dispatch_id, resumed, remote_agent?,
  outcome?}` on the calling principal with its chain, plus the `message` it
  sent (`{secure_input_sent: true}` for an answer to `input-required`); the
  test chat's `debug.sub_agents` pairs them by `dispatch_id`. Analytics count
  them as sub-agent dispatches.
- **Validation** (save and publish): `task` required, `bind` as for a
  sub-agent route (and the same trusted-gate rule for state binds), `a2a` keys
  `card_url`/`auth`/`finish`/`budget` only, the card URL's shape and grant,
  `auth.kind` and its keys (exactly one of the plain or sealed secret,
  `client_id` for client credentials), `finish.schema` through
  `FinishContract::new` (required on publish), and `budget.seconds` in 1–900.

### SSRF

`aiplane_core::server::outbound_guard`, `Policy::agent` — the same guard
`fetch_url`, `load_image_url`, the host-JWT JWKS fetch and a manager's inbox
channel use. The card URL, the endpoint the card names and the OAuth token URL
are each resolved before every connection; every address must pass, and the
request goes out on a client pinned to exactly those addresses
(`resolve_to_addrs`), with redirects off, so a second DNS answer cannot swap
in a private one.
- Always refused: unspecified, link-local (169.254.0.0/16 with the metadata
  endpoint, fe80::/10), broadcast, multicast, and their IPv4-mapped forms.
- Refused unless `$AIPLANE_ALLOW_PRIVATE_NETWORKS=true` (`Config.network`,
  environment only like `$AIPLANE_TRUSTED_PROXIES`): loopback, RFC 1918,
  100.64.0.0/10, fc00::/7 and plain `http`.

The MCP OAuth flow goes through the same guard under a policy of its own
(`Policy::mcp_oauth`), which allows private ranges on purpose: an admin
curates the MCP catalog.

Tests: `agents/run/tests/a2a.rs` against a wiremock A2A peer (card, JSON-RPC
endpoint and token endpoint: what is sent and not, the bearer header, the
checked result, the credential stored sealed only; violating, prose, failed
and free-text `input-required` answers each `incomplete`; polling; structured
`input-required` paused and answered, the value at the peer and nowhere else;
a loopback peer refused without the switch; a revoked grant; a slow peer; one
OAuth token and one card fetch for two dispatches; an injection flagged).
Pure halves in `a2a_client/tests.rs`, `guard.rs`, `card.rs`; the grant cap and
the sealed save over HTTP in `tests/it/system_principals.rs` and
`tests/it/agents.rs`.
