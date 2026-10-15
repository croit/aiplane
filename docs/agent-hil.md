# People in the loop

An agent run can wait for a person: staff approving a tool call, staff
answering a hand-off, or the visitor typing a secure value. All of it is one
durable pause — `chat_turn_suspensions`, the same mechanism every approval in
the product uses ([`tools-rbac.md`](tools-rbac.md#suspend-and-resume)) — plus
what agent runs add on top: pauses nested through sub-agents, who may answer
which kind, an inbox, and notifications.

## Suspend and resume

`FeedbackHub` parks a call only while the turn lives in memory, which is
enough for `ask_user` and browser control but not for an approval that may
take hours. A suspension is stored:

```sql
CREATE TABLE chat_turn_suspensions (
    turn_id      TEXT PRIMARY KEY NOT NULL REFERENCES chat_turns(id) ON DELETE CASCADE,
    request_id   TEXT NOT NULL UNIQUE,   -- an answer to an earlier pause cannot settle a later one
    kind         TEXT NOT NULL,          -- 'approval' | 'secure_input' | 'human_answer'
    message      TEXT,                   -- what the tool wants shown
    tool_call    TEXT NOT NULL,          -- JSON {id, name, args}
    tail         TEXT NOT NULL,          -- JSON: this turn's round messages so far
    budget_used  TEXT NOT NULL,          -- JSON {rounds, seconds, tokens, tree?}
    child_turn   TEXT,                   -- set when the pause is inside a sub-agent run
    on_timeout   TEXT NOT NULL,          -- the fallback the kind allows
    expires_at   TEXT NOT NULL,          -- compared after parsing, not as a string
    created_at   TEXT NOT NULL,
    run_context  TEXT,                   -- a dispatch's {route, route_binds}; a hand-off
    notified_at  TEXT                    -- set once the pause was announced
) STRICT;
```

Pausing writes the row and sets `chat_turns.status = 'suspended'` in one
transaction, emits the `chat_json` event `suspended` and frees the worker.
Nothing waits in memory: every level of a paused run is rebuilt from the rows
on whichever process gets the decision, so a restart loses nothing.

**Kinds.** `session_core::db::SuspensionKind` is the registry: `approval`,
`secure_input`, `human_answer`. Each kind says which decisions it offers
(`options()`: allow once / deny for an approval, value / deny for the others),
who answers it in an agent conversation (`answered_by()`: `Participant` for
`secure_input`, `Staff` for the other two), and what its timeout falls back to
(`timeout_fallback()`). The driver applies that fallback whatever the tool
asked: an approval only ever times out to deny, and a value kind cannot time
out to `allow_once`, which it does not offer — every expiry is a deny.
`SuspendRequest::approval(timeout)`, `secure_input(message, timeout)` and
`human_answer(question, timeout, context)` build the requests.

**Which runs pause.** `ToolContext.suspend` is `Available` on the chat path
and on every headless run: every agent run (`headless::drive` turns it on for
an agent actor) through both `drive_opened` entry points, and a person's
scheduled or webhook run. `/v1` cannot pause.

**The main agent's turn.** `drive_opened` returns `AgentReply { status:
suspended, answer: None, suspension: Some(view) }` when the turn paused. The
output filter runs only on a terminal answer. The pause is audited as
`run_suspended` (`{session_id, turn_id, request_id, kind, tool, child_turn,
expires_at}`) on the running principal with its chain.

### Nested pauses

A tool inside a sub-agent run pauses the child turn as usual.
`forward_request` sees the child's row once its drive returns, records the
dispatch's `run_context` on it (`{route, route_binds}`: the bound values exist
nowhere else) and answers with a suspend envelope whose `child` names the
child turn and its deadline. The parent's driver checks that the child is a
paused run of *this* turn (`parent_turn_id`), then pauses the parent with the
child's kind, message and deadline and `child_turn` set. Every level up to the
conversation's turn does the same, so the conversation's turn mirrors the
innermost request. Each row has its own `request_id`; the one a client sees
and answers is the conversation's.

**One decision, innermost first** (`agents::resume`).
- `claim` walks `child_turn` from the conversation's turn down (at most
  `RunChain`'s `MAX_DEPTH` levels, each level's session naming the level above
  as its parent), checks `request_id` against the conversation's row, the
  decision against the innermost kind's options and the answerer against
  `answered_by()`, then rechecks the exact pause snapshots and session links
  inside one write transaction. It claims the innermost request and every
  ancestor by their expected request IDs and commits them together. A changed
  or missing ancestor, invalid decision or conflicting claim leaves every
  suspension and turn status intact.
- `run_claimed` rebuilds each level from its session (owner, pinned version; a
  test conversation's draft) and its parent's pause (the chain is
  `RunChain::root` plus one `enter` per level, `CallSite` = the parent's turn
  and waiting call; an A2A caller is read back from `a2a_contexts`), resumes
  the innermost run with the decision, turns its outcome into the
  `forward_request` result exactly as a first dispatch would
  (`router::dispatch_result`, which also writes `sub_agent_finished`), and
  resumes each parent with that result (`ResumeFrom.child_result`: the waiting
  call is answered, not run again). A child that asks again pauses every level
  again on the new request. A level that cannot be rebuilt (the agent disabled
  since) errors every claimed turn.
- The resume is audited as `run_resumed` (`{session_id, turn_id, request_id,
  kind, decision, answered_by, waiting_turn}`, with `actor_id` for staff).
- A resumed run speaks the conversation's recorded language, whoever gives the
  decision ([`agent-runs.md`](agent-runs.md#where-runs-live)).

**Secure values.** A `value` goes to the requesting tool through
`ToolContext.suspend = Decided(SecureInput, Value)` and nowhere else.
`Decision`'s `Debug` prints `<redacted>`; the events carry the decision's
shape only (`secure_input_received: true`); for a `secure_input` the driver
replaces every repetition of the value in the tool's result with `[secure
input withheld]` before the result is stored or reaches the model, so a
careless tool cannot leak it either. One rule, `Redaction::decided`, says what
is withheld: a secure input's value, never an approval or a person's answer.

**Messages behind a decision.** A conversation that waits holds its place: a
visitor's `POST /api/v0/embed/messages` stores the user turn and answers `202
{turn_id: null, user_turn_id, placement: "queued"}` (`"started"` otherwise);
a second is `409 turn_in_progress`. The event stream's `snapshot` lists it
in `waiting_turn_ids`, and once the resumed turn is terminal `run_claimed` runs
it as the next turn. `run_turn` and the test chat refuse instead
(`AgentRunError::DecisionPending`, `409 decision_pending`), and so does an A2A
message into a waiting context ([`agent-a2a.md`](agent-a2a.md)).

**Expiry.** `run_sessions::expired_run_suspensions` lists the conversations'
own rows (principal-owned, no `parent_turn_id`) past their deadline;
`agents::resume::resume_expired`, called by the chat's 30-second suspension
sweeper, resumes each with its stored fallback, answered by `timeout`,
skipping a conversation whose claim is held. The chat's own sweep skips
principal-owned runs.

**Retention.** The retention sweep keeps a conversation with a pending
suspension, however idle; it goes on a later sweep once the decision or the
expiry settled it.

### Answering

| Who | Route | Answers |
|---|---|---|
| The visitor | `POST /api/v0/embed/resume` `{request_id, decision, value?}` (unknown fields refused), visitor token, admitted like a message (rates and budget) | a `secure_input` only: an approval answers `403 decision_for_staff`. Also `409 not_suspended` (nothing waiting, or a stale `request_id`), `400 decision_not_offered`, `409 turn_in_progress`. The refusals a visitor can act on are Fluent strings (`agent-embed-decision-for-staff`, `agent-embed-not-waiting`) |
| An A2A caller | `SendMessage` with the task's `taskId` | a `secure_input` only ([`agent-a2a.md`](agent-a2a.md#serving-an-agent-over-a2a)) |
| Staff | `POST /api/v0/agents/{id}/conversations/{session}/turns/{turn}/resume` `{decision, value?, request_id?}`, `can_manage_agents` plus a `write` share (or admin) | approvals and hand-offs in any of the agent's conversations; a `secure_input` only in a test conversation, where the manager plays the visitor (`403 decision_for_visitor` otherwise) |
| Anyone the inbox shows the item to | `POST /api/v0/agents/inbox/{id}/answer` | [below](#the-inbox) |

Every answer that resumes a turn answers `202 {turn_id}` once the turn runs
again, in the background under the conversation's claim, so the visitor's
stream (or the test chat's) shows it running. The staff route and the inbox
end in the same `agents::resume::claim` / `run_claimed` with
`ResumedBy::Staff` (`json_inbox::resume_as_staff`), so `run_resumed` names the
answerer as `actor_id`.

**What a visitor sees.** A suspended turn keeps `suspension` in the snapshot
and `GET /api/v0/embed/session` as `SuspensionView::for_participant`:
`{request_id, kind, message?, options, expires_at}`, no `tool` and no
`tool_call_id`; `options` is empty for a request staff answer. The event
stream ends with a `suspended` frame of the same shape in place of `idle`
when the conversation waits or the running turn pauses. The widget shows a
masked field for a secure input and a waiting notice for a staff request,
re-attaching every 10 s until the answer arrives
([`embed.md`](embed.md#when-the-agent-asks-a-person)).

**The test chat.** A test conversation (version 0) streams the same
`suspended` frame; its debug view carries the pause's hand-off as `handoff`.
A test-chat pause never reaches the inbox (`inbox::staff_item` skips version
0); the manager answers it in the test chat through the staff route
([`ui.md`](ui.md#agent-builder)).

A budget snapshot may include `tree`: the original aggregate and local limits,
observed rounds, seconds and tokens, and whether upstream usage was reported.
It uses the same suspension JSON column. Ordinary snapshots omit that optional
field. The connection-level claim and creation helpers remain generic chat
operations; child ownership is resolved in the agent layer.

## Per-tool approval

`agents::approval`. `tool_resources.<tool>.permission` decides whether a call
pauses for staff: `always_ask` wraps the tool in `AskFirst`, `always_allow`
runs it as granted. Without a `permission` a tool asks first exactly when it
is known to change something: `Tool::changes_state()` is `true` only for an
MCP tool its server marks destructive and not read-only (built-in tools say
nothing, so they are `false`). `tool_resources.<tool>.approval_timeout` (a
duration, default `1h`) is how long the approval may take; an approval nobody
gives is a deny. The inbox item shows the innermost call's tool and arguments,
and the request's `message` when the tool says what the call would do.

`AskFirst` is the same wrapper a person's MCP tool in `ask` mode gets in chat
([`connectors.md`](connectors.md#tool-modes-always-ask-off)), so a person's
scheduled or webhook run pauses on such a tool too.

## Hand-offs

`agents::human`. A route with `human` is a target like a sub-agent:

```yaml
routes:
  person:
    when: { slot: issue, set: true }
    description: "A billing question the agent cannot settle"   # the question staff see
    human:
      notify: [push, slack, discord]   # absent = every channel
      inbox: support                   # a label the inbox shows
      timeout: 30m                     # default 30m
      transcript: false                # default false
```

Two ways in:
- `request_human(question)`, a synthetic tool offered to the main agent
  whenever the spec has a `human` route. It needs an open human route
  (`router.order` first, else name order) and answers `no_open_route` with
  what is missing otherwise. *Chosen:* the argument is the question for staff,
  not a free-form reason, and it takes no other key.
- `forward_request` picking a human route. The route's `description` is the
  question, else the visitor's last message.

Either pauses the call as `human_answer` with the question as `message` and a
hand-off stored in the pause's `run_context`: `{handoff: {route, question,
visitor_message, slots, lang, inbox, notify, transcript?}}`. `slots` is the
model's view — a value only where the model wrote it, `set_by` otherwise, and
the slot's `label` from its `description` when it has one — so a verifier's
value never reaches the inbox. The transcript (the last 20 turns, each cut to
2000 characters) goes along only with `transcript: true`. The hand-off is
audited as `human_handoff` with the run chain.

**The answer goes through the main agent.** *Chosen over verbatim:* the staff
answer is the waiting call's result (`{answered: true, answer, note}`) and the
model passes it on in the visitor's language. The visitor and the staff member
need not share a language, the conversation stays one the model continues, and
the answer still passes the output filter (a staff answer is trusted text, so
identifiers staff quote pass). A staff `deny` is a tool error the model
explains.

**Nobody answers.** When a hand-off's deadline passes, `run_claimed` does not
ask the model: the waiting call is settled as unanswered and the turn ends
with `agent-human-no-answer` in the conversation's recorded language. A
message queued behind it runs afterwards as usual.

## The inbox

Private organization Dot conversations require current organization membership
and Write access for staff settlement. Draft pauses stay inline in Test chat.
Published private approvals, human answers and secure input also appear in the
Inbox after exact private session, captured source and original version-row
proof. Read, Respond and administrator recovery without membership expose no
such items. These conversations permit writer staff to supply secure input;
ordinary published visitor conversations retain participant-only secure input.
Organization announcements remain in the web interface and do not dispatch
push or external channel messages. See [private organization
authority](agent-runs.md#organization-private-conversation-authority).

`agents::inbox`, `aiplane-api::pages::json_inbox`. An item is a
conversation's own pause (`run_sessions::pending_suspensions`; a sub-agent's
pause shows through its conversation's):

- an ordinary agent conversation's `approval` or `human_answer`, outside the test chat,
  for whoever holds access to the agent (`effective_access`): an admin or a
  manager with a `read` or `write` share (`standing: manager`), or a `respond`
  share (`standing: responder`);
- a person's own paused conversation — a scheduled or webhook run — for its
  owner only (`standing: owner`), answered through the chat's own resume
  (`pages::chat::resume_turn`), so it continues as the owner's chat with the
  owner's tools; its expiry is the chat sweeper's. The SPA shows such a run as
  waiting.

A visitor's secure input is never anyone's item.

**Responders** are `respond` shares ([`agents.md`](agents.md#shares)): users or
groups who answer an agent's approvals and hand-offs and see nothing else of
it. They need no `can_manage_agents`. A responder gets the item and its
minimal context — question, hand-off context, an approval's tool and
arguments, the agent's display name. Every other agent route refuses them
(`403` without the agent-management permission, `404` with it), and so does
the staff resume route. Managed on `/api/v0/agents/{id}/shares` like every
share, and audited as one.

**API** (`aiplane-api::pages::json_inbox`). The inbox routes need a session
only; everyone may ask, most see nothing.

| Method | Path | Who | Purpose |
|---|---|---|---|
| GET | `/api/v0/agents/inbox` | session | `{items, count, answers}`: `answers` is whether the viewer answers for at least one published agent (`inbox::answers_for_published`, the standing rule above over published agents); each `{id (request_id), kind, standing, agent?, session_id, turn_id, title?, question?, call?: {name, arguments}, detail?, context?, options, created_at, expires_at}` |
| POST | `/api/v0/agents/inbox/{id}/answer` | who may answer it | `{decision, value?}`; `202 {turn_id}`, the turn runs in the background and the visitor gets it on their stream. `404 inbox_item_not_found` for anyone else, `409 not_suspended` / `turn_in_progress`, `400 decision_not_offered` |
| GET | `/api/v0/agents/inbox/events` | session | SSE: `inbox {count, answers}` on attach and whenever the set changes (checked every 3 s; `answers` is read when the stream attaches), keep-alive comments, ends after 10 minutes for `EventSource` to reconnect |
| GET/POST | `/api/v0/agents/{id}/channels` | read / write share | list without URL; add `{kind: slack\|discord, name, url, details?, lang?}`; `422 invalid_webhook_url`, `409 channel_name_taken` |
| DELETE | `/api/v0/agents/{id}/channels/{channel_id}` | write share | `204` |

The SPA's `/inbox`, the sidebar entry `answers` decides, and the suspension
card are in [`ui.md`](ui.md#inbox).

## Notifications

Once per pause (`chat_turn_suspensions.notified_at`, set with `WHERE
notified_at IS NULL`; a new pause is a new row), off the turn's path
(`inbox::announce_in_background`), when an agent conversation's turn pauses
(`drive_opened_from`, so a resumed turn that pauses again notifies again) or
a person's headless run does:

- **Web Push** to everyone who may answer: users holding a share that takes
  effect for them, directly or through a group, or the run's owner. Admins
  without a share are not notified — they may answer everything and would be
  told everything. Title and body from the catalog in each subscription's
  language, through the one fan-out (`push::send_to_user`, which also cuts and
  prunes); the link is `/inbox?item=<request_id>`.
- **Slack and Discord** incoming webhooks (`agent_notify_channels`,
  `db::agent_channels`, `aiplane_agents::notify_channels`), narrowed by the
  hand-off route's `notify` list. The URL is the credential: sealed at rest,
  never returned by the API, never in a log line or
  an event; only its host is kept in clear.

**Who configured a channel decides where it may post**
(`notify_channels::Reach`, from `inbox::channel_reach`). An admin's channel is
the operator's own choice: any `http(s)` URL, a private or loopback host
included (an internal Discord relay), posted through the operator's client
(`AppState::http`). Anyone else's must be `https` on `hooks.slack.com` or
`discord.com`/`discordapp.com` `/api/webhooks/…`, checked on save with
`outbound_guard::check_url` and posted on every send through
`outbound_guard::pin` with `Policy::agent` (public hosts only unless
`$AIPLANE_ALLOW_PRIVATE_NETWORKS`), so a manager's channel never posts into
the gateway's network. The standing is decided at send time from the
channel's `created_by` as that person stands today: a channel whose creator
is not an admin any more, or is gone, posts as a manager's — no column
records it.

A message holds the agent, the kind and the absolute inbox link
(`public_url`); with the channel's `details` on, also the question or the tool
name (cut to 300 characters). No visitor message, transcript or slot value is
ever sent. Slack text is escaped, Discord gets `allowed_mentions: {parse:
[]}`. A channel has its own catalog `lang`. A failed post is logged and
skipped. Answers are given in the inbox only — there is no reply from a Slack
or Discord thread, and no mail channel.

Tests: `agents/run/tests/suspend.rs` (secure input to the tool and nowhere in
the database; a paused sub-agent pausing its caller and one staff decision
resuming both; an expired approval denied; restart survival; a refused second
message), `agents/run/tests/hil.rs` (approval allowed, denied and timed out;
`always_allow`; a hand-off answered through the model, with its context and
audit; the unanswered hand-off in German with no model call; a human route
through `forward_request`; one Slack post per pause on the named channels; a
person's paused run in their own inbox only), `tests/it/embed/suspend.rs` and
`tests/it/embed/hil.rs` (the HTTP surface, every table and log line checked
for a secure value, what a responder cannot reach, `respond` shares, channel
management without the URL, a scheduled run resumed from its owner's inbox),
`tests/it/turn_suspension.rs` (a person's chat).
