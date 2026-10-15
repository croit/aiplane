# The activity log

A complete, tamper-evident record of everything an agent does: enough to
reconstruct any conversation, decision and model exchange afterwards, with no
secret in it. The log is `aiplane-agents::db::agent_audit`; the runtime's door
to it is `aiplane-runtime::agents::audit`. The analytics an agent's managers
see are derived from it ([below](#analytics)).

**One writer.** Every event goes through `agent_audit::append`, reached from a
run through `agents::audit::record_event` (and the wrappers `record` and
`ToolContext::audit`) or, for a management change, on the change's own
transaction. `activity_events_are_written_only_through_the_activity_log`
([`testing.md`](testing.md#architecture-tests)) fails on any other `INSERT`,
`UPDATE` or `DELETE` of `agent_audit` and on any other call of its writers.

**One log.** Decision events (`tool_call`, `route_decision`,
`run_suspended`, …), management events (grants, shares, publishes) and
content events (model exchanges, tool results, turns) are events of the same
table, with the same columns and the same chains. Analytics, the test-chat
debug view and the evaluation read the decision events from it.

## Columns

Every event carries its correlation ids as columns, not only in `detail`:
`agent_id` (the main agent at the root of the run — a sub-agent's events
belong to the agent whose conversation they happened in), `principal_id` (who
acted: the running frame), `version`, `conversation_id` (the root
conversation), `session_id` (the run's own session: a sub-agent's child
session), `turn_id`, `round`, `call_id`, `visitor_id`, `caller_id` (the A2A
caller), `actor_id` (the person behind a management change or a staff
decision), `duration_ms`, `chain` (the serialized `RunChain`), `created_at`,
`kind` and `detail`, plus the hash-chain columns `chain_key`, `seq`,
`prev_hash`, `hash` and `key_id`.

## Hash chains

*Chosen:* one chain per conversation, sub-agent runs included (`chain_key =
conversation:<root session>`), and one per agent for everything outside a
conversation (`agent:<principal id>`: management changes, refused visitors,
sweeps, anchors). Per conversation rather than per agent because retention
removes conversations, and a chain must go whole or not at all; and because
parallel conversations then never contend for one chain head.

**No writer picks the chain.** `agent_audit::append` resolves it from the
event's own `session_id`, following a child session's `parent_turn_id` up to
the root conversation (whose owner is also the event's `agent_id` when no run
chain says so), so a state write, a host identity or an A2A task names only
the session it happened in and lands where the rest of the conversation is; an
event with no session belongs to its run chain's root, or else to the agent's
own chain.

Within a chain events count up from 1 (`seq`); each stores the hash of the one
before (`prev_hash`) and its own `hash` = HMAC-SHA256 over its canonical JSON —
every column except `hash` and the rowid, keys sorted, no whitespace, `chain`
and `detail` as the exact stored text — under the log key its `key_id` names.
A unique index on `(chain_key, seq)` refuses a fork. Every event has a chain,
so `verify` reports an event outside every chain as inserted outside the
gateway.

**The log key.** No new secret: the key is derived from the gateway's at-rest
key (`Crypto`, `$AIPLANE_ENCRYPTION_KEY` or the session secret) as
HMAC-SHA256(at-rest key, `croit-aiplane/activity-log-chain/v1`), and
`Crypto::activity_keys` returns the ring, which holds the current key. Each event stores the `key_id` (the first 16 hex digits
of the key's SHA-256: a fingerprint, not the key) it was signed with, and
`verify` picks that key from the ring. Replacing `$AIPLANE_ENCRYPTION_KEY`
keeps no old key — sealed secrets stop opening too — and `verify` then reports the older
events as signed with a key the gateway does not hold.

The ring is installed process-wide when the gateway's state is built
(`AppState::new`, `with_crypto`), because every writer, the management changes
in `aiplane-agents` included, extends the same chains — and those record on a
caller's transaction with a connection, not the gateway's state, so passing
the ring explicitly would thread it through every management accessor and its
handler. The ring is shared (`Arc<[ActivityKey]>`) and each key holds its keyed
HMAC state, so a signature costs no key schedule. An event written by a
process without a ring (a unit test, a CLI) carries `key_id = unkeyed` and a
plain SHA-256; where a ring is installed, `verify` refuses it, so rewriting a
chain "unkeyed" does not pass either.

**Anchors.** When a turn of a conversation ends — paused or answered, at the
end of `drive_opened_from`, after the output filter — the conversation chain's
head (`seq`, `hash`) is written into the agent's own chain as a
`chain_anchored` event, read and written in one write transaction
(`agent_audit::anchor_conversation`, through `agents::audit::anchor`, bounded
like any event; a failure is logged and the conversation's newest events stay
unanchored until the next turn). Voice calls anchor after each call too.

## Verifying

`agent_audit::verify(agent)` walks the agent's own chain first, collecting the
latest anchor per conversation and the sweep markers, then every conversation
chain, in batches of 256, and reports the first thing that does not hold:
- a `seq` gap (an event removed from the middle);
- a `prev_hash` that is not the previous hash (an event inserted or relinked);
- a `hash` that does not match the content under its key (an event changed, or
  re-hashed without the key), an unkeyed or unknown-key event;
- a conversation chain that ends before its anchored `seq`, or whose event at
  that `seq` is not the anchored one (its tail was cut or rewritten);
- an anchored chain that is gone without a sweep marker (a conversation's log
  deleted inside its retention);
- an event outside every chain.

It also reports `unanchored` (conversation events newer than their chain's
latest anchor) and the agent chain's `head` (`seq`, `hash`). The anchored
event is read by its `(chain_key, seq)` index, not found by walking.

**Watermarks** (`agent_audit::verification`). *Chosen over re-walking every
time:* a chain that checked out is remembered in `activity_verified` — its
`seq` and `hash` and, for the agent's own chain, the anchors and sweep markers
read so far — signed under the log key like an event (`key_id`, `mac`).
`verify` resumes every chain from its watermark, so a check hashes what was
written since the last one (`checked`), while `events` still counts everything
the chains hold; `verify_full` (`?full=true`) walks every chain from its
start. A watermark that is unsigned, signed with a key the ring lacks or not
matching its signature — one the database alone moved — is ignored, and so is
one whose event is gone or does not have the stored hash (a chain cut or
rewritten at it); either way the chain is walked whole. The cost: a change
*below* a watermark (an event rewritten with its hash column untouched, one
deleted from the middle) shows only on a full walk — run `?full=true`
periodically, as with the pinned `head`. The sweep deletes a conversation
chain's watermark with the chain.

**Residual limits.** Someone holding the at-rest key (or the session secret it
is derived from) can forge anything — the key protects against a
database-only attacker, not against the gateway's operator. A database-only
attacker can still cut the agent chain's own tail together with the
conversation events those last anchors covered, and delete events written
after the last anchor (`unanchored`); both show only against a copy of the
`head` kept outside the gateway, which is why `verify` returns it — an
operator who exports the log or pins the head regularly (cron, SIEM) bounds
what can disappear unnoticed to what was written since. Deleting a whole
agent's rows, chain and anchors alike, leaves nothing to compare against
except that pinned head.

## Writing: synchronous, bounded, fail closed

*Chosen over a bounded queue:* every event is written before the run moves on,
in a write transaction of its own (`BEGIN IMMEDIATE`, so concurrent writers to
one chain queue on SQLite's lock rather than racing for its head) or — for a
management change and a state write — on the change's own transaction, so the
change and its event commit together or not at all. Either way the
transaction is a `db::WriteTx`, which only `WriteTx::begin` (`BEGIN
IMMEDIATE`) makes and which `agent_audit::append` requires: a chain head, a
slot's old value or a rate window is read under the write lock it is then
written under, never in a deferred transaction that takes the lock only at its
first write. WAL with `synchronous = NORMAL` makes a commit a page write, not
an fsync, so this costs a turn about a millisecond per event; nothing waits in
memory, so nothing is lost on a crash or a shutdown and there is no queue to
flush.

- **Bound:** a run event may take at most `agents::audit::WRITE_BOUND` = 5 s
  (lock wait included).
- **Fail closed:** an event of an agent run that could not be written marks
  the run (`AgentRun::mark_log_failed`); the driver stops it before the next
  model round and before a batch of tool calls runs, errors its turn with
  `agents::audit::LOG_UNAVAILABLE`, and a turn that had already answered when
  its `turn_finished` could not be written is errored after the fact, so the
  visitor never gets an answer the log does not show. The failure is logged at
  `error` with the event's correlation ids. Outside a run (a refused visitor,
  a sweep marker) the failure is logged and the request goes on.

Personal Dot tool admission and results also fail closed without an `AgentRun`:
`ToolError::AuditUnavailable` and the runner's typed failure cause terminate the
turn before another model round. A failed admission prevents that tool's remote
call. A result-log failure can occur after a remote action has completed; the
runner stops waiting tool phases, settles the turn's tool rows and does not
retry the action. Calls already admitted or dispatched concurrently cannot be
rolled back by an audit failure. Approval resume applies the same termination
rule.

## What is recorded

As low in the code as it allows, so nothing reaches around it:

| Event | Written by | Detail |
|---|---|---|
| `llm_exchange` | the driver's round loop (`openai_driver/exchange.rs`), around the upstream call, whichever way the round ends | `purpose: round`, `round`, `model`, `real_model`, `backend`, `request` or, after a turn's first round, `request_delta` ([Storage](#storage)) — the body exactly as sent: system message, messages, tool offer, parameters, but for what the log never keeps (below) —, `response` (`status`, `content`, `reasoning`, `tool_calls`, `finish_reason`, `usage`), `latency_ms`, `error` (no backend, transport, non-2xx with the full body, stall, loop), `cancelled` |
| `llm_exchange` | a side call of the run ([below](#model-calls-outside-the-round-loop)) | `purpose`, `model`, `backend`, `request`, `response`, `latency_ms`, `error`; a constrained choice also its `answer` |
| `scope_decision` | the topic guard | `verdict` (`in_scope`, `out_of_scope`, `failed`), `topics`, `model`, `error` |
| `tool_call` | the call policy; personal Dot calls admitted through the current live tool source | `decision`, `policy` ([`agent-runs.md`](agent-runs.md#every-call-decided-and-audited)); personal MCP admission can include the selected connection's typed owner, connector key, connection ID, credential generation and binding generation |
| `tool_result` | the tool runner (`execute_tool_call`), for every call including an unregistered tool and a refused repeat; the resume path for a denied call and a sub-agent's result | `tool`, `arguments` (as the model wrote them; `{redacted: true}` for a tool that declares `sensitive_args`), `status` (`completed`, `failed`, `invalid_args`, `timed_out`, `unregistered`, `refused_repeated`, `denied`, `answered_by_sub_agent`), `result` (the tool's whole answer before injection screening and before the prompt's byte budget trims it), `injection` (`policy`, `signals`); `duration_ms` |
| `state_written` | `agent_state::put`, on the write's transaction, in the chain of the written session's root conversation | `slot`, `old` (`value`, `provenance`, `set_at`, or `null`), `new`, `provenance`, `set_at` |
| `turn_started` / `turn_finished` | `headless::drive`, for every agent turn (main and sub-agent, resumed too) | the message the turn answers (a visitor's, or a sub-agent's task), or `resumed: true`; `status`, `answer`, `error`, `outcome` (a contracted run's `RunOutcome`) |
| `route_decision`, `sub_agent_dispatched` / `_finished`, `loop_iteration` / `loop_finished` | the router ([`agent-runs.md`](agent-runs.md#the-router)) | every route's gate, the route picked and the `method`; an A2A dispatch also the `message` it sent |
| `run_suspended`, `run_resumed`, `human_handoff` | the pause and resume paths ([`agent-hil.md`](agent-hil.md)) | `run_resumed` also carries a staff `answer` to a hand-off, and only `secure_input_received: true` for a secure input |
| `verifier_outcome`, `host_identity` | the verifiers ([`agent-visitors.md`](agent-visitors.md#identity-verifiers)) | never a code, an address or a claim value |
| `output_blocked` | the output filter | the withheld `original` and what was `delivered` |
| `limit_refused` | the visitor gate ([`agent-visitors.md`](agent-visitors.md#refusals)) | folded per minute |
| `a2a_task` | the A2A server | never the text |
| `injection_detected` | the injection scan | `tool`, `call_id`, `policy`, `signals`, never the matched text |
| management kinds | the agent DB modules, on the change's transaction | `principal_created`, `principal_disabled`, `grant_added`, `grant_removed`, `token_issued`, `token_revoked`, `agent_created`, `agent_draft_updated` (with `restored` / `revoking` for an undo), `agent_published`, `agent_live_version_set`, `agent_share_set`, `agent_share_removed`, `agent_deleted`, `embed_key_created`, `embed_key_revoked`, `channel_created`, `channel_deleted` |
| `assist_suggested` | the prompt assistant, in the agent's own chain | `action`, `scenario` / `template` or `field` / `text`, `model`, `usage`, `latency_ms`, `offered`, `dropped`, `error` ([`agent-builder.md`](agent-builder.md#prompt-assistant)) |
| `conversations_swept`, `activity_swept`, `chain_checkpoint` | the retention sweep ([Retention](#retention)) | counts and chain bounds only |
| `chain_anchored` | the end of every turn (`drive_opened_from`) | `chain_key`, `seq`, `hash` of the conversation chain's head |

For a personal Dot, the optional `tool_call.identity` records the selected
connection for that attempted call, correlated with its existing `call_id` and
outcome. It does not assert that the provider accepted the action. The final
MCP check either validates that same identity or refuses execution; a changed
selection cannot substitute another account. Resuming the recorded call keeps
its original admission event. The identity contains no tokens, provider email,
secret arguments or generic tool metadata. Owned MCP identities also contain
the actual catalogue generation. Shared catalogue identities contain only
their connector key and catalogue generation; they have no owned connection
ID. These facts bind the attempted call to the admitted resource configuration.

`for_principal` — the decision trail the agent's GET, the test-chat debug view
and the evaluation read — leaves out the content kinds (`AuditKind::is_content`:
`llm_exchange`, `tool_result`, `turn_started`, `turn_finished`); they are read
through the activity API.

### Model calls outside the round loop

They go through the same door as an `llm_exchange` with their own `purpose`:
one record (`server::side_call::SideExchange`) and one writer
(`agents::audit::RunLog::record`), recorded for an agent's run only —
`RunLog::of` is `None` for a person's turn, so a person's chat records nothing
here. The purposes: `route_classifier` and `scope_guard`
(`agents::model_call`), `compaction_summary` (an agent conversation's
compaction, in that conversation's chain), `rubric_judge` (an evaluation's
judge, in the case's test conversation), `vision_fallback` (each call to the
fallback vision model that describes an image a tool returned for a model that
cannot see — `capabilities::maybe_replace_image_content` returns its calls —
with the tool call's `call_id`), `transcription` and `speech` (voice). Each
records `model`, `backend`, the `request` exactly as sent (an image included),
`response` (`status`, `body`), `latency_ms` and `error`; a constrained choice
also its `answer`.

### Secrets never enter it

The log redacts at its one choke point: `agent_audit::append` applies
`agent_audit::Redaction` to every event's detail before it is hashed, by event
kind — the arguments of a call to a tool that declares `sensitive_args` become
`{redacted: true}` in a `tool_result` or `tool_call`, and in every tool call an
`llm_exchange` carries (the answer's `tool_calls`, and the assistant
`tool_calls` of every later request or request delta); a turn's secure input
becomes `[secure input withheld]` in an event of any kind. No writer redacts
for itself, so none can forget to: each event of a run carries the run's
`Redaction` (`ToolContext::redaction`, attached by `ToolContext::audit` /
`audit_event` and `RunLog`), which the run collects as it goes — the driver
adds the secure input a turn resumes with, and the runner, the exchange log
and the resume path note every tool they meet that declares its arguments
sensitive (`AgentRun::note_sensitive`).

A one-time code reaches the verifier through `Suspend::Decided(SecureInput,
Value)` only — the decision carries the kind of pause it settles, and one
rule, `Redaction::decided`, says what is withheld: a secure input's value,
never an approval or a person's answer (which `run_resumed` records). The same
rule withholds it from what goes elsewhere than the log (`Redaction::withhold`:
the resumed call's result the model reads, the verifier's connector answer).
A stored request therefore differs from the one sent exactly there. The A2A
credential is sealed in the spec and sent only as a header, which no event
records. Tokens, embed keys and client secrets are hashed or sealed where they
are stored and never part of an event.
`activity::a_whole_run_is_one_hash_chain_that_reconstructs_it_and_holds_no_secret`
(`agents/run/tests/activity.rs`) runs a conversation through a closed gate, an
OTP verifier fed a known code, a sub-agent, an external A2A agent with a sealed
bearer token and a hand-off to a person, then greps every text column of every
table for the code and the token.

**Server logs.** Every agent turn runs in a `tracing` span `agent_turn` with
`agent`, `principal`, `version`, `conversation`, `session`, `turn`, `visitor`,
`caller` and `depth`, and every tool call in a `tool_call` span with `tool` and
`call_id`, so a log line joins the events it belongs to.

## Storage

A model round's request carries the whole conversation so far, so the log does
not store it whole every round (`agent_audit::exchange`): the first round of a
turn keeps its `request`; every later round keeps a `request_delta` against the
round before it — `prev` (that exchange's event id), `keep` (how many of its
messages this request starts with), the `messages` after them, the `system`
message only when it changed, `tools` only when the offer changed (`null` when
it was dropped), and `rest` (model, stream and sampling parameters). A request
that shares nothing with the previous one beyond the system message is stored
whole. A `data:` URL of at least 4 KiB (a base64 image) is stored once per
chain in `activity_blobs` by its SHA-256, and the detail holds
`activity-blob:sha256:<hash>` in its place; the reference is inside the event's
signed hash, a blob whose content does not match its hash is not served, and
the sweep deletes a chain's blobs with it. The activity API's page and export
serve every exchange as the whole request it stood for
(`agent_audit::Reconstructor`, which follows `prev` back to a whole request and
puts the blobs back). So a turn of *r* rounds stores its prompt about once plus
what each round added, plus every tool result once.

SQLite stores large rows on overflow pages, which the hourly sweep frees whole
chain by chain; the file does not shrink without a `VACUUM`, but freed pages
are reused. The correlation and chain columns sit after `detail` in the row,
and reading a column behind an overflowing `detail` means walking its overflow
chain, so every query that does not need the payload is answered from an index
alone (a test checks the plans). Indexes: `(chain_key, seq)` unique — chain
order, the fork guard and verification; `(chain_key, seq, hash)` — the chain
head every append reads; `(agent_id)` and `(conversation_id)` — the API pages
by rowid within either, which those single-column indexes are ordered by;
`(agent_id, chain_key, conversation_id, created_at)` — the retention sweep;
`(principal_id, kind, created_at)` — the decision trail and analytics.

## Retention

`publish.audit_retention_days` (default **365**, read from the live version
like `retention_days`, and at least `retention_days` so a conversation's log
always outlives the conversation): the hourly retention sweep
([`agent-visitors.md`](agent-visitors.md#retention)) deletes the *whole* chain
of a conversation that does not exist any more and whose newest event is older
than that — never part of a chain — and appends an `activity_swept {chain_key,
events, before}` marker per chain to the agent's own chain, in the same
transaction, so the removal is recorded with it.

The agent's own chain honours the same retention, so it does not grow for ever
with anchors, sweep markers and management events: before the conversation
chains, the sweep cuts the chain's *prefix* written before the cutoff
(`agent_audit::cut_agent_chain`), in one transaction with a signed
`chain_checkpoint` appended at its head — `base_seq` and `base_hash` (the last
removed event), `removed`, `seqs` (the removed range), `from`/`to` (when they
were written), `before`, and `anchors`: the latest anchor of every
conversation among them that was not swept, so a live conversation stays
guarded. Anchors of swept conversations go with the prefix. `verify` starts
the agent chain at the newest checkpoint (after `base_seq`, expecting
`base_hash`) and seeds its anchors from it; a checkpoint met further along adds
its carried anchors without replacing a newer one. Removing more of the prefix
shows as a missing event, and removing the checkpoint as a gap in the chain.

## Access and API

`aiplane-api::pages::json_agent_activity`: the agent-management permission
plus a `read` share (admins hold one on every agent). The log holds whole
conversations, so a responder, whose `respond` share answers hand-offs only,
cannot read it (`403`, or `404` with the permission).

| Method | Path | Purpose |
|---|---|---|
| GET | `/api/v0/agents/{id}/activity?conversation=&kind=&from=&to=&cursor=&order=&limit=` | A page of events (`limit` 1–500, default 100, and at most ~4 MiB of detail), newest first or `order=asc`; `kind` is a comma list; `from`/`to` RFC 3339 or `YYYY-MM-DD`; `{events, next_cursor, order}`. A conversation's events include its sub-agent runs |
| GET | `/api/v0/agents/{id}/activity/export?conversation=&kind=&from=&to=` | Every matching event, oldest first, one JSON object per line (`application/x-ndjson`), streamed a ~1 MiB batch at a time with backpressure |
| GET | `/api/v0/agents/{id}/activity/verify?full=` | From each chain's watermark, or from the start with `full=true`; `{ok, chains, events, checked, unanchored, head: {chain_key, seq, hash} \| null, broken: {chain_key, seq, event_id, reason} \| null}` — keep `head` outside the gateway to detect a log cut back to an earlier one |

An event reads `{cursor, id, kind, ts, principal_id, actor_id, agent_id,
version, conversation_id, session_id, turn_id, round, call_id, visitor_id,
caller_id, duration_ms, run_chain, detail, chain_key, seq, prev_hash, hash}`.
A sub-agent's activity shows in the log of the agent whose conversation it ran
in, not in its own. The SPA's *Activity* tab is in [`ui.md`](ui.md#agent-builder).

Tests: `aiplane-agents`' `db/agent_audit/tests.rs` (correlation columns,
management events on the caller's transaction, every way a chain breaks, 40
parallel writers to one conversation, paging and the byte cap, the sweep taking
whole chains only); `agents/run/tests/activity.rs` (a whole run, a changed
event found by `verify`, six parallel conversations each with its complete
sequence, a run whose log cannot be written stopped before the model is asked,
a lost `tool_result` stopping the next round); `agents/retention.rs`;
`tests/it/agent_activity.rs` (the API: timeline, filters, cursor, export,
verify, who may read it); `web/src/lib/agent-activity.test.ts`.

## Analytics

What an agent did over a period, for its managers, derived from rows that
exist anyway — the activity log's decision events, `usage_events` and the chat
tables. No second event store.

**`GET /api/v0/agents/{id}/analytics?from=&to=&version=`** (`read` share,
admins always; the same 403/404 rules as the other agent routes). `from` and
`to` are RFC 3339 instants or `YYYY-MM-DD` UTC days; a day as `to` includes
that whole day. Default: the last 30 days. Longer than 366 days, `from >= to`,
an unparseable bound or `version < 1` is a 400 that says what to change.
`version` narrows to one published version.

**Response** — counts and route, slot and reason names only; no visitor
content, no session or visitor ids:
`{from, to, version, currency, conversations, turns, sub_agents: {dispatched,
finished, incomplete, incomplete_by_reason: {kind: n}}, gate_refusals: {total,
by_route: {route: n}, by_missing_slot: [{route, slot, count}]}, routes_chosen:
{route: n}, output_blocks: {total, by_action}, limit_refusals: {total,
by_kind}, human_handoffs, usage: {requests, prompt_tokens, completion_tokens,
tokens, cost}, daily: [{day, conversations, turns, tokens, cost, refusals}]}`.
`daily` has one UTC-day bucket for every day of the range, quiet ones as
zeros.

**Where each number comes from.**
- `conversations`: root conversations (`parent_turn_id IS NULL`) the agent's
  principal owns, by `created_at`. `turns`: their `user` turns, by the turn's
  `created_at`. A conversation the retention sweep deleted is gone from both.
- `routes_chosen`: `route_decision` events with a `picked` route.
  `gate_refusals`: `route_decision` events with `reason: no_open_route`; each
  closed route counts once in `by_route` and each of its unmet slots once in
  `by_missing_slot`. A decision the classifier declined (`picked` null, any
  other reason) is not a gate refusal.
- `sub_agents`: `sub_agent_dispatched` and `sub_agent_finished` events (A2A and
  loop dispatches included); `incomplete_by_reason` is `outcome.reason.kind`.
- `output_blocks`: `output_blocked`, by `action` (`redacted`, `withheld`).
- `limit_refusals`: `limit_refused`, by `limit` (`visitor_rate`, `ip_rate`,
  `budget`).
- `human_handoffs`: `human_handoff` events (`AuditKind::HumanHandoff`).
- `usage`: `usage_events` with `agent_id` = the agent, which includes the
  sub-agents' calls and the side calls. Tokens are `total_tokens`, or prompt +
  completion when that is missing. Needs `[usage] enabled`.

**What is never counted.** Builder test conversations (`agent_version = 0`;
their events and usage rows carry the draft version in the chain's main
frame), another agent's rows, and a run of this agent as somebody else's
sub-agent (the chain's main frame names the other agent).

**Version filter.** A row's version is the main frame of its serialized call
chain (`frames[0].version`); conversations use `chat_sessions.agent_version`.
`limit_refused` events have no chain, because a refusal happens before any
version runs, so they are left out while a version is selected (the SPA says
so).

**Aggregated in SQL.** Conversations, turns, usage and the kinds that are only
counted are `GROUP BY substr(created_at, 1, 10)` (the UTC day) queries. The
version filter reads the chain with `json_extract`; the exact range compares on
`rtrim(created_at, 'Z')`, because RFC 3339 text with fractional seconds of
varying length orders correctly only without its `Z` (`db::window_key`);
whole-day bounds alongside keep the indexes in use. Only the events whose
`detail` carries the numbers (route decisions, sub-agent outcomes, output
blocks, limit refusals) are fetched.

The SPA's *Analytics* tab is in [`ui.md`](ui.md#agent-builder). Tests:
`crates/aiplane/tests/it/agent_analytics.rs` seeds two agents, two versions, a
test conversation, out-of-range rows and a visitor's text and id, and asserts
every number exactly, the daily series, the version filter, no leakage between
agents, no visitor content, and the share rules.
