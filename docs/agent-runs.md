# Agent runs

How one turn of an agent's conversation runs: as the agent's principal, on
the chat driver, under the spec's gates, binds and budgets. The spec itself
is [`agent-spec.md`](agent-spec.md); pausing for people is
[`agent-hil.md`](agent-hil.md). The generic loop machinery an agent run
shares with a person's chat — run budgets, the finish contract, injection
scanning, suspend and resume, the repeated-call guard — is described once, in
[`tools-rbac.md`](tools-rbac.md#the-tool-call-loop).

## Where runs live

Agent conversations reuse the chat substrate: `chat_sessions` and
`chat_turns`, the session worker registry, the `chat_json` SSE protocol and
compaction. They are owned by the principal, not by a person:

```sql
chat_sessions:
    user_id        TEXT NULL REFERENCES users(id) ON DELETE CASCADE
    principal_id   TEXT NULL REFERENCES system_principals(id) ON DELETE CASCADE
    parent_turn_id TEXT NULL     -- a sub-agent run: the turn that dispatched it
    agent_version  INTEGER NULL  -- the version the conversation is pinned to; 0 = a draft
    visitor_id     TEXT NULL REFERENCES visitor_sessions(id) ON DELETE SET NULL
    lang           TEXT NULL     -- the language the conversation is held in
    CHECK ((user_id IS NULL) != (principal_id IS NULL))
    CHECK (principal_id IS NULL OR shared = 0)
```

- **Separation from people.** Every person-facing query lists sessions by
  `user_id`, so an agent conversation never shows in anyone's chat list. The
  second CHECK keeps it out of the "anyone with the link" read path. Managers
  read an agent's conversations through its activity log
  ([`agent-activity-log.md`](agent-activity-log.md)) and answer its pauses in
  the inbox.
- **session-core stays owner-agnostic.** It knows a person's conversation by
  `user_id` and treats any other owner as opaque. The principal-owned side —
  `SessionOwner`, `create_principal_session` (`NewRunSession`),
  `get_principal_session`, `session_owner`, the agent pause sweep and the
  inbox reads — is `aiplane_agents::db::run_sessions`.
- **Sub-agent runs** are child sessions owned by the sub-agent's principal and
  linked through `parent_turn_id`; they are not chats of the owner.
  `parent_turn_id` has no foreign key, so a child run stays an auditable record
  when the parent turn is gone.
- **Versions.** A conversation records the version it started on and keeps
  it ([`agents.md`](agents.md#agent-definition)). A draft run is recorded as
  version `0` (`DRAFT_VERSION`): a test conversation is never continued as a
  visitor's (`MissingVersion`), nor a visitor's as a test (`unknown_session`).
- **Language.** Every new turn records the language it was asked in as
  `chat_sessions.lang`, when the caller knows it (the public endpoint and an
  A2A task from the request, `run_turn` from `AgentTurn.lang`). That column is
  the run's only source of a language: the output filter's fallback texts, an
  unanswered hand-off's message, the `lang` a hand-off records, a verifier's
  and the A2A client's prompts all read it (`ToolContext::conversation_lang`,
  through the chain's root conversation, so a routed sub-agent speaks its
  caller's). A resume passes none, so staff answering from the inbox in their
  own language do not change it; a conversation that recorded none is English.

## One run, one value

Everything that sets an agent's run apart from a person's turn travels in one
`AgentRun` (`aiplane_runtime::agent_run`): the `SystemPrincipal` it acts as,
its `Arc<RunChain>`, its finish contract, budget and injection scan, and the
spec's `AgentSurface`. `AgentRun::new(principal, chain)` is the only
constructor and returns `MismatchedRun` when the chain's running frame is not
that principal, so a run that acts as one agent and audits as another cannot
be built.

A turn's `Actor` is `Person { id, roles }` or `Agent(Arc<AgentRun>)`.
`DriveParams` and `TurnFacts` take one, and `build_tool_context` derives
`ToolContext.principal` from it, so the principal and the run cannot disagree.
The run then lives in `ToolContext.agent` (the driver reads it back through
`OpenAiDriver::agent()`), and every question "is this an agent run" asks that
one value: the call policy, the injection audit, the usage row's chain
(`ctx.chain()`), and whether `headless::drive` announces a person's pause.

There is no second driver. The round loop asks the turn's `TurnPolicy`
(`openai_driver/turn_policy.rs`) — `Chat`, `Agent(&AgentRun)` or `Persona`
(the agent architect, [`agent-builder.md`](agent-builder.md#agent-architect))
— for the system message, the tool offer, the model access, the budget, the
topic guard and how the turn ends, and holds no agent branch of its own. An
agent run therefore never reads a person's conversation overlay or "off"
switches (`chat_session_tools`); a run with no spec surface gets the
turn-discipline rule as its system message and every grant as its offer.

### Entry points

- **`drive_opened(state, profile, turn)`** (and `drive_opened_from`, the same
  with a resume) drives a turn whose rows exist: the visitor's user turn and an
  `in_progress` assistant turn, named by an `OpenedTurn {agent_id, version,
  session_id, turn_id, visitor_id, caller, lang}`. Every way into an agent run
  ends here: the public endpoint and the A2A server through the installed
  `AgentTurnRunner` (`LiveAgentRunner`, set by `main.rs` with
  `RamaState::with_agent_runner`), the test chat and evaluations through
  `agents::run::draft`, a resume through `agents::resume`. It records the
  turn's language, runs the output filter on a terminal answer, audits a
  pause, announces it to the inbox and anchors the activity log.
- **`run_turn(state, AgentTurn {agent_id, session_id, message, visitor_id,
  lang})`** opens the rows itself and calls the same `drive_opened`; it
  returns `AgentReply {session_id, turn_id, status, answer, error,
  suspension}`. A `session_id` must belong to that agent (`UnknownSession`
  otherwise); a new conversation runs the live version. `run_turn_with` takes
  `RunOptions {now, classifier, spend}`: the clock gates and slot writes read,
  a `RouteClassifier` to use in place of the model classifier, and a loop
  route's `SpendMeter`.
- **`RunProfile::load_from(state, id, SpecSource, Role, options)`** loads what
  a run needs: `{principal, version, model, budget, finish, injection,
  surface, output_filter}`. `SpecSource` is `Live`, `Pinned(version)` or
  `Draft(spec)`; `load` and `load_version` are the first two, and only
  `agents::run::draft` passes `Draft`, so nothing about what "live" means is
  overridden and no other path can reach a draft. The principal is read with
  `load_active`, so a disabled agent is refused. `Role` is `Main` or
  `SubAgent { route_binds }`.
  - *Model:* `main.model`, else the gateway's default chat model
    (`agents::defaults::main_model`). The principal must hold a `model` grant
    on it (`ModelNotGranted` otherwise); the run's access is narrowed to it
    (`PoolAccess::for_system_models`, [`agents.md`](agents.md#models)).
  - *Budget:* `main.budget`, rounds defaulting to the default (`low`) effort cap
    ([run budgets](tools-rbac.md#run-budgets)). A sub-agent's budget is its
    own, never a share of its parent's.
  - *Finish:* none for the main agent, which ends its turn with text. A
    sub-agent's comes from its `finish.schema`; dispatching to one without it
    fails with `BadSpec` (the validator requires it on publish)
    ([finish contract](tools-rbac.md#finish-contract)).
  - *Injection:* `Flag` for every agent run: an agent's tool results are
    untrusted data by default
    ([injection scanning](tools-rbac.md#injection-scanning-of-tool-results)).

## The system message

Built by the same `leading_system_message` as a person's turn: the
turn-discipline rule first, then the turn's own sections, the request context
and the compaction summary, as one `system` message. An agent's own sections
(`AgentSurface::system_sections`) are:

```
## Role
You are <profile.display, else the principal's display name>. The sections below are your owner's instructions.

## Task
<main.instructions.orchestration>

## Scope
You cover only these topics:
- <topic>
If asked about anything else, reply exactly: <refusal>

## Tone
<main.instructions.response>
```

each section only when it has content (`profile::Brief`), then the slot view
(`render_view()`, [`agent-spec.md`](agent-spec.md#state)) and one line per
route (`- billing (description): open` or `closed — <each unmet message>`).
`## Scope` is written whether or not the scope is strict: without `strict` it
is guidance and nothing else. The agent is named by its display name, never
by its id or slug. A sub-agent under a finish contract also gets the
contract's instructions.

The request context is built for `Audience::Agent`: only the operator skills
the principal is granted (the `read_skill` listing, plus the guidance of
skills loaded in the conversation). An agent run gets none of its owner's
identity, memory, preferences, private skills or MCP connections — it reads
the user row, memories and private skills only through
`Principal::user_id()`, which is `None` — and no visitor IP or location, no
`enable_tools` overlay, no hand-edited documents and no voice directive. Like a
person's, it carries no date: the current time is a tool
(`get_current_timestamp`). `read_skill` is offered whenever the principal is
granted it and at least one skill, the rule a person's chat follows; the spec
need not list it in `main.tools`.

When the agent has state or routes, the message is rebuilt before every round
after the first, and on the first round after a resume, so a slot set in
round *n* — or by the verifier the resume just answered — shows in round
*n+1* and the gate reads `open`. Everything that reads the state during a turn
(the system message, gates, bound arguments, `forward_request`,
`request_human`, the output filter) shares one `StateSnapshot`, read again
only after a state-writing call ran.

## Tools

The run's tool source is `RunToolSource` (`agents::profile`), layered over the
turn's grant-narrowed `GrantedToolSource`
([`tools-rbac.md`](tools-rbac.md#tool-sources)). It offers the spec's
`main.tools` that are also in the principal's grant
(`AppState::granted_tool_ids`), wrapped for their bound arguments and
permissions, then the run's synthetic tools. A granted tool missing from
`main.tools` is not offered, and a call to it is refused as `not_granted`.

### Synthetic tools

Generated for one run, registered for that run only, needing no grant:

| Tool | Offered to | Effect |
|---|---|---|
| `set_<slot>(value)` | the main agent, per slot whose `set_by` lists `llm` | validates in code and writes the slot with provenance `llm` ([`agent-spec.md`](agent-spec.md#state)) |
| `forward_request()` | the main agent, when the spec has routes | evaluates the router over the open gates ([below](#the-router)) and runs the chosen route's target |
| `request_human(question)` | the main agent, when a `human` route exists | hands the conversation to a person on an open human route and waits for the answer ([`agent-hil.md`](agent-hil.md#hand-offs)) |
| `verify_<id>_request_code()`, `verify_<id>_submit_code()`, `verify_<id>()` | the main agent, per complete verifier | the identity verifiers ([`agent-visitors.md`](agent-visitors.md#identity-verifiers)); no arguments |
| `finish(result)` | runs under a finish contract (routed sub-agents) | ends the run; `result` is checked against the finish schema |

The call policy sees them as offered, so each call is decided and audited as
`granted`.

**Order within a round.** A model batches calls, so `set_issue` and
`forward_request` often arrive in one round. A round's calls normally run
concurrently, which would let the forward read the state before the write
landed. Each synthetic tool is therefore tagged with a `ToolPhase` where the
run builds it (`AgentSurface`; the run's `finish` tool is the other tool that
leaves the default), and the runner (`execute_tool_calls`) runs the phases in
turn:

1. **Writes state**: `set_<slot>` and every verifier tool, one at a time in the
   order the model made the calls (a lookup reads the slots a `set_<slot>`
   before it wrote).
2. **Concurrent**: every other tool, in parallel. A bound argument may be read
   from state, so these run after the writers.
3. **Acts on state**: `forward_request` and `request_human`, one at a time in
   call order, on the state the round left.
4. **Terminal**: `finish`. The driver runs it only as the one call of its
   round, and a call that succeeds ends the run.

Each result still answers its own `tool_call_id`, in call order. Outside an
agent run every tool is `Concurrent`.

### Every call decided and audited

`openai_driver/call_policy.rs` decides each call of an agent run before
anything executes and writes one `tool_call` event per call, attributed to the
running principal, with the chain: `{tool, call_id, turn_id, session_id,
decision, policy}`. `decision` is `allowed` or `denied`; `policy` is
`granted`, `not_granted`, `unknown_tool` or `disabled_in_conversation`.

**A system principal never auto-enables.** The chat path runs a registered
tool the model calls without its schema and turns it on for the conversation.
For a system principal that call is refused as `not_granted`: it holds
exactly its grants.

### Bound arguments

`agents::bind::BoundTool` wraps a granted tool that has a `bind`. It binds
exactly the parameters its `tool_resources.<tool>.bind` maps; nothing binds
by name. Mapped parameters are dropped from `properties` and `required`, and
the gateway's value overwrites the model's on every call. A `state.` source is
read when the call runs, and the call is refused while the slot is unset; a
`route.<name>` source is filled from the dispatching route's values; a
`const` is fixed. A tool that declares a subject parameter it does not bind is
withheld (`bind::WithheldTool`): not offered, and refused when called. The
approval gate of a tool's `permission` sits outside the bound arguments, so an
approved call still gets the gateway's values, and a withheld tool is refused
without asking anyone.

## The call chain

```rust
// aiplane-core, server/run_chain.rs
pub struct RunChain {
    pub root_session: String,           // the conversation the run tree started in
    pub visitor_id: Option<String>,
    pub caller: Option<RemoteCaller>,   // an A2A caller; serialized only when set
    frames: Vec<Frame>,                 // the main agent, then each sub-agent hop
}
pub struct Frame {
    pub principal_id: String,
    pub name: String,                   // so an event reads without a join and outlives the principal
    pub version: Option<i64>,           // None for a principal without a published spec
    pub via: Option<CallSite>,          // None on the main agent
}
pub struct CallSite { pub turn_id: String, pub tool_call_id: String }
```

`RunChain` lives in `aiplane-core` next to `Principal`, because the usage and
MCP audit rows that serialize it are written there. `RunChain::root` starts a
chain; `enter` appends a sub-agent and refuses a fourth level
(`EnterError::TooDeep`, `MAX_DEPTH` = 3) and an agent already in the chain
(`EnterError::Cycle`) — at run time, on top of the validator's graph check.
`CallSite` names the turn as well as the call because the child session's
`parent_turn_id` needs it.

The chain rides in the run's `AgentRun` (`ToolContext::chain()`), and these
records carry it serialized: every activity log event of the run, every usage
row (`usage_events.chain`, plus `agent_id`, the main agent at its root), and
every `mcp_tool_audit` row. A sub-agent's calls carry the extended chain.
Usage rows of a run carry `principal_kind = 'system'` and the principal that
made the call in `user_id`, so a sub-agent's call reads `user_id = <sub-agent>,
agent_id = <main agent>`.

## The router

`forward_request()` (`agents::router`) takes **no arguments** — any key is
refused by name — and decides from state:

1. Every route's gate is evaluated. Only open routes are candidates. With
   none, the result is `{forwarded: false, reason: "no_open_route", routes:
   [{route, missing: [Unmet]}]}`, which tells the model what is missing.
2. One open route is picked:
   - a `rules` router takes the first open route in `router.order`, else the
     first open route by name — deterministic;
   - exactly one open route is taken without asking a model;
   - otherwise (`classifier`, or no router) a side call on `router.model`, else
     the main run's model, with `response_format: json_schema` and `route`
     constrained to `enum: <open routes>`. It is sent the route descriptions
     and the model's slot view, never trusted values. The answer is checked in
     code again, so a closed, unknown or malformed answer forwards nothing
     (`no_route_chosen`). The classifier can only pick among routes whose gate
     is already open.
3. The route's target runs, reached only through the `OpenRoute` that
   `RouteGates::open` returns: a sub-agent ([below](#dispatch-to-a-sub-agent)),
   a person ([`agent-hil.md`](agent-hil.md#hand-offs)), an external A2A agent
   ([`agent-a2a.md`](agent-a2a.md#external-agents-as-route-targets)) or a
   [loop](#loop-routes).

Every decision is a `route_decision` event (`{routes: [{route, gate}],
picked, method, reason?}`, `method` one of `rules`, `only_open`,
`classifier`). The classifier's call is a usage row of the run and an
`llm_exchange` with `purpose: route_classifier`.

A route's target is its one key besides `when`, `description`, `task` and
`bind`: `agent`, `human`, `a2a` or `loop`. Everything a kind needs lives under
that key, its own `finish` and `budget` included, so a builder that does not
know a kind still finds a route's shared keys where they always are. A route
naming two targets is one issue at `routes.<r>`.

### Dispatch to a sub-agent

1. Render `task` from the main agent's state (`bind::render_task`). A
   placeholder may read any valid slot, verifier slots included: the task goes
   to the sub-agent, not back to the visitor or the main model. A placeholder
   that cannot be filled answers `task_incomplete`.
2. Resolve the route's `bind` against the state.
3. Load the sub-agent's **live** version with those values (`Role::SubAgent`)
   and enter the chain (`CallSite { turn_id, tool_call_id }`; the call id is
   `ToolContext::call_id`, set by the runner on each call's context).
4. Open a child session owned by the sub-agent's principal (`parent_turn_id` =
   the main turn) with the task as the user turn, and drive it with its own
   budget, contract, grants and injection policy (`run_child`, shared with
   the loop route).
5. Return `{forwarded: true, route, sub_agent, outcome: RunOutcome, note}` as
   the tool result, which the main agent's `Flag` policy screens like any
   other.

The sub-agent sees its system message and the task, never the transcript. The
call waits at most 15 minutes (`FORWARD_TIMEOUT`). A sub-agent the draft
dispatches to in the test chat runs its own live version. Each dispatch writes
`sub_agent_dispatched` and `sub_agent_finished` (`{route, sub_agent,
sub_agent_id, version, session_id, turn_id, outcome?}`) on the calling
principal with its chain. A sub-agent run that pauses pauses its caller
([`agent-hil.md`](agent-hil.md#nested-pauses)).

### Loop routes

A route kind that drafts, critiques and revises (Google ADK's LoopAgent
pattern): a worker sub-agent and a critic sub-agent take turns until the
critic accepts. `agents::router::loop_route`; validation in
`agents/spec/route_kinds.rs`.

```yaml
routes:
  offer:
    when: { slot: issue, set: true }
    task: "Write an offer for: {issue}"
    bind: { customer: state.verified.customer_id }   # passed to both, as route.<name>
    loop:
      worker: <agent id>                  # finishes with the result
      critic: <agent id>                  # finishes with { accepted: boolean, feedback?: string }
      max_iterations: 3                   # default 3, at most 10
      budget: { seconds: 300, tokens: 60000 }   # caps the sum of every child run; optional
```

- **One iteration** is a worker child run, then a critic child run, each
  started exactly as a sub-agent route starts one (`run_child`): its own
  principal, live version, grants, finish contract and `main.budget`, a child
  session under the main turn, the call chain extended. Neither sees the
  transcript. The worker's first task is the route's rendered `task`; from the
  second iteration on, that task plus its previous result and the critic's
  `feedback`, both marked as data. The critic's task is the original task and
  the worker's result, with the instruction to set `accepted` and say in
  `feedback` what to change.
- **Stopping.** The loop ends when the critic's result has `accepted: true`
  (`stopped: accepted`), after `max_iterations`, when the route's budget is
  spent before the next child run (`budget`), or when a child run ends
  `incomplete` (`worker_incomplete`, `critic_incomplete`).
- **The route budget caps the sum.** `seconds` counts from the start of the
  loop; `tokens` counts every round of every child run and of anything they
  dispatch: `RunOptions.spend` carries a `SpendMeter` into the children's
  runs, and the driver adds each round's tokens to the meter of the agent run
  it drives. Each child's own budget is tightened to what is left
  (`Budget::capped`), so a child ends on its own limit rather than being cut
  off; the loop checks the remainder before every child run. Without `budget`
  only the children's own budgets and the 15-minute dispatch ceiling apply.
- **What returns to the main agent** is the worker's last result:
  `{forwarded: true, route, loop: {worker, critic, iterations, accepted,
  stopped}, outcome, note}`. `outcome` is the worker's last `finished` result
  even when the critic never accepted it (`accepted: false` says so), the
  worker's own `incomplete` when it did not finish, or `incomplete` with
  `tokens_exhausted` / `seconds_exhausted` when the budget ran out before any
  result.
- **A child that pauses** (an approval, a secure input) is withdrawn
  (`cancel_suspended_turn`) and counts as `incomplete`. *Chosen:* the next
  step of the loop needs its result now, and resuming a loop mid-iteration
  would need the loop's own state stored with the pause. Agents with
  `always_ask` tools make poor workers and critics.
- **Audit.** Every child run writes `sub_agent_dispatched` and
  `sub_agent_finished` with `loop: {route, iteration, role: worker|critic}`;
  each critic verdict writes `loop_iteration` (`{route, iteration, accepted,
  feedback}`); the end writes `loop_finished` (`{route, worker, critic,
  iterations, accepted, stopped, tokens}`).
- **Validation** (save and publish): `task` required, `bind` as for a
  sub-agent route; `loop` keys `worker`, `critic`, `max_iterations`, `budget`
  only. `worker` and `critic` go through the sub-agent checks (existing agent,
  not the agent itself, live to publish) and the graph check, so a loop that
  reaches the main agent again is a cycle at `routes.<r>.loop.<role>` and counts
  toward the depth of 3, and a worker or critic that binds a tool demands a
  trusted gate. The worker and the critic differ. Against the critic's live
  version, its `finish.schema` must list `accepted` as a required `boolean`
  (`feedback`, if declared, a `string`); against both, the route passes exactly
  the `route.<name>` values either binds; on publish the worker needs a
  `finish.schema`. `max_iterations` is 1–10, `budget.seconds` 1–900,
  `budget.tokens` ≥ 1.

## Topic guard

A rule in the instructions ("do not answer questions outside your scope") is
a request the model may ignore, and without topics it cannot even tell what is
out of scope. A strict `scope` makes it a gateway decision.

Under `scope.strict`, a main agent's turn asks the guard (`agents::topic_guard`)
before its first model call (`TurnPolicy::guard_topic`, the one place, in
`run_one_turn`) — never on a resume, where the message was judged when the
turn began, and never for a routed sub-agent, whose input is a task, not a
visitor's message.

- **Input:** the topics, the latest visitor message and the exchange before it
  (the previous visitor message and answer), each clipped to 2000 characters.
  The exchange is what lets "and what about the price?" after an in-scope
  question stay in scope; older history is left out to keep the call small.
  Greetings, thanks and "what can you do?" count as in scope.
- **Call:** `agents::model_call::ModelCall`, the mechanism the route
  classifier uses too: one side call (`side_call::ask_json`) to
  `classifier_model` (else the main model) under the principal's model grant
  and the agent's budget, `response_format` an enum of `in_scope` /
  `out_of_scope`, and the answer checked again in code. It is a usage row of
  the run, its tokens count against the turn's `main.budget.tokens`, and it is
  an `llm_exchange` with `purpose: scope_guard`.
- **Verdict:** recorded as `scope_decision` (`verdict`, `topics`, `model`,
  `error`). Only a clean `in_scope` lets the turn continue (trust rule 1: the
  model may only deny). `out_of_scope` makes the owner's `refusal`, verbatim
  and untranslated, the turn's answer, and the main model is not called. The
  answer still passes the output filter.
- *Chosen: fail closed.* When the guard cannot decide (no model, a non-2xx, an
  answer that is not a verdict), the visitor gets the refusal and the
  `scope_decision` carries `verdict: failed` with the `error`. A strict scope
  promises that off-topic messages never reach the main model; failing open
  would break that promise exactly when the guard model is down and nobody is
  watching. The cost is an in-scope visitor refused during an outage, which
  the error in the log makes visible.

The test chat's debug view carries the turn's verdict as `scope`.

## Output filter

`agents/output_filter.rs` is the whole filter; `drive_opened` calls
`guard_answer` once, on the main agent's final answer, so every entry point
is covered.

- **Spec.** `publish.output_filter.patterns` (name → regex, validated) and
  `publish.output_filter.action` (`withhold`, the default, or `redact`). No
  patterns, no filter: the answer is untouched.
- **Rule.** Every match of every pattern in the answer must also occur, as the
  same pattern's match, in the turn's *trusted text*. Anything else, the
  visitor's own message included, is untraceable. Trusted text is:
  - the conversation's slots not written by `llm`;
  - the outputs of the turn's **successful** tool calls. An errored call
    contributes nothing: its message is the tool talking about its input ("no
    invoice RE-99999 found"). *Errored* is the call's real outcome: the runner
    marks a result failed (`ToolResultRecord::failed`) when the tool returned
    an error (an MCP `isError` included), rejected its arguments, timed out,
    was unregistered, or never ran (refused as a repeat, over the budget, a
    second suspend request in a round), and the driver stores those rows as
    `errored` — which is also what the chat UI shows as a failed call. The
    `set_<slot>` calls are left out because they echo model-written values.
  - *Echo rule.* An identifier in a call's output does not vouch for itself
    when that call's model-supplied argument values could have supplied it:
    the model chose it, the data did not. Otherwise the model could launder a
    visitor's claim by passing it to a tool that repeats it. Both sides are
    compared as lowercase letters and digits only, so reformatting does not
    hide an echo (`{"invoice": 999999}`, `"re 999 999"` or `"RE-"` +
    `"999999"` all supply `RE-999999`). An identifier with digit runs of at
    least 4 digits is supplied when *every* such run occurs in some argument:
    the runs tell one customer from another, a prefix is the pattern's. One
    without such runs is supplied when its whole alphanumeric core occurs in
    one argument. Shorter runs are ignored because they occur in almost any
    argument by chance, so a lookup by `{"year": 2026}` still vouches for the
    `RE-2026-0042` it returned. *Chosen over* a pattern's capture group: that
    asks every spec author to mark the distinctive part, and a spec without one
    would fall back to the weaker verbatim test. The stored arguments are the
    model's own; a bound argument is filled in by the gateway afterwards, so a
    tool repeating a bound value is not an echo (and the value came from a
    verified slot, which is trusted on its own anyway).
  - *Sub-agents.* A `forward_request` result is not trusted text: a sub-agent
    repeats its task (which can carry an `llm` slot) as readily as a model
    repeats a visitor. The successful tool calls of every sub-agent run below
    the turn (`chat_sessions.parent_turn_id`, recursively, loop workers and
    critics included) count by the same rule. A sub-agent's answer can thus
    name an invoice its own lookup returned, but not one it only read in its
    task. *Chosen over* trusting the identifiers of the sub-agent's finish
    result that also occur in those tool outputs: that only narrows trust to
    what the sub-agent chose to mention, which adds no safety, and walking the
    tool calls needs no second notion of "the result".
  - *People.* A staff answer to a hand-off (`{answered: true}` from a human
    route or `request_human`) is trusted whole: a person read the request and
    wrote it.
- **Withhold** replaces the whole answer with the `agent-output-withheld`
  catalog message; **redact** replaces each offending identifier with
  `agent-output-redacted`. The stored turn is overwritten the same way, so a
  replayed conversation never shows the blocked text. Both are in the
  conversation's recorded language.
- **Audit.** `output_blocked` on the agent's principal with the run chain and
  `{action, session_id, turn_id, patterns, original, delivered}`: the pattern
  name of each offending occurrence, and the withheld answer for the agent's
  managers. If the trusted text cannot be read the answer is withheld (fail
  closed) and the event carries `error`.
- **No early peek.** The turn row is terminal before the filter has ruled, so
  the public endpoint and the A2A server treat a turn as unfinished while its
  claim on the conversation is held ([`agent-visitors.md`](agent-visitors.md#the-event-stream)).
- **Limits.** The filter matches text, not meaning: an identifier the model
  rewrites (`RE 123456`) escapes a pattern that does not allow for it, and a
  tool the agent calls that returns another customer's data makes that data
  trusted — grant tools bound to the verified subject for that. The echo rule
  errs towards withholding: a tool that returns an identifier whose digit runs
  happen to occur in its arguments (an invoice numbered like the customer)
  does not vouch for it. A tool that answers an error as a successful result
  (`{"error": …}` without failing) is not recognised as failed; the echo rule
  still catches the number the model passed it. A successful lookup by a
  number the visitor gave does not confirm that number either, only what the
  lookup returned beside it. Only this turn's calls count: an identifier a
  tool returned in an earlier turn must be looked up again. An A2A route's
  remote answer is never trusted text.

Public answers are buffered for this filter: token-by-token streaming and a
filter that sees the whole answer cannot both hold, and the filter wins for
untrusted audiences.

## Stopping a turn

An agent turn runs on a worker of the session worker registry, like a person's
chat turn: `agents::embed::claim(workers, principal, session, turn)` registers
it, keyed by the principal that owns the conversation, and one claim per
conversation keeps two messages from running at once (`409
turn_in_progress`). `SessionWorkers::cancel_turn` stops only the turn holding
the claim. A message queued behind a decision runs under the resumed turn's
claim, which `SessionWorkers::hand_over` passes on. `headless::drive` runs a
root turn on the worker that claimed it (its cancel flag and channel); any
other agent turn — a sub-agent's child run, or a root turn nobody claimed —
registers a worker of its own while it runs and still stops by its root
conversation's flag, so a cancel reaches sub-agent runs too. An A2A
`CancelTask` and shutdown (`cancel_all`) set it. `spawn_guarded` errors a
turn whose runner left it unfinished or panicked, before the claim drops.

## Side calls

Every one-off model call beside a conversation — the session title, the
compaction summary, the feedback form's fields, the topic guard, the route
classifier, the rubric judge and the prompt assistant — goes through
`aiplane-runtime::server::side_call` (`ask_text`, `ask_json`). It lives in
`aiplane-runtime` because that is the lowest crate with `RamaState` and
`model_route`, and the highest every caller can see. One call:

- resolves the model as a chat turn does (`model_route::route_target`) under
  the access the caller gives (`PoolAccess::all()` for the title and the
  feedback form, whose model is the person's own chat model or the
  operator's; the principal's model grant for an agent's calls);
- checks the payer's spend limits *before* the model is called: a person's
  through `Enforcer::check_for_model`, an agent's through
  `Enforcer::check_agent` (the owner budget of its live version plus the
  operator's `system` rules, for the agent at the root of the run, which the
  row is booked to). A pool exempt from enforcement is never refused. A
  refused call is `SideCallError::OverBudget`: the title and compaction skip,
  the feedback form answers `429`, the assistant `429` with `Retry-After`, the
  topic guard fails closed;
- sends `chat_template_kwargs.enable_thinking: false` always — a reasoning
  model (Qwen on SGLang) asked for JSON otherwise sometimes spends the whole
  answer thinking and returns empty content — and Qwen3's `/no_think` after
  the input where the caller asks (title, compaction, feedback), with the
  caller's temperature and `max_tokens`;
- reads the answer capped (`MODEL_ANSWER_BYTES`) within one timeout (title
  15 s, choice 30 s, compaction and judge 60 s, assistant 60/120 s, feedback
  120 s); `ask_json` strips a code fence and finds the first object in
  surrounding prose; an upstream error reads `upstream <status>: <first 160
  characters of the body>`;
- writes the payer's usage row (`source` the caller's: `chat` for a person's
  title, feedback and assistant call, the turn's own source for compaction,
  `agent` for an agent's calls, joined to its run).

For an agent's run, each side call is also an `llm_exchange` in the activity
log with its own `purpose` ([`agent-activity-log.md`](agent-activity-log.md#what-is-recorded)).

Tests: `agents/run/tests.rs` (the support example end to end on wiremock
upstreams — both models and the ERP as a wiremock MCP server: the closed
gate's feedback, a verifier write, `forward_request` dispatching billing, the
bound `customer_id` overriding the model's, the finish result answered; plus
the classifier, `order`, separate budgets, a cycle, an injection in a
sub-agent result, the audit chain, models), `agents/run/tests/output_filter.rs`,
`agents/run/tests/loop_route.rs`, `agents/run/tests/topic_guard.rs`,
`agents/output_filter.rs`, `agents/topic_guard.rs`, `agents/profile.rs`.
