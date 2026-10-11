# What the builder runs on

The server side of the agent builder: the test chat against the draft, the
list of what a manager may grant, evaluation, the setup assistant's spec
conventions, the prompt assistant, the agent architect and draft revisions.
The screens themselves are in [`ui.md`](ui.md#agent-builder).

## Test chat

A manager talks to the agent's **draft**, or an organization Dot’s published
version, in a private conversation that streams like any other, and reads per turn what a visitor never sees
(`aiplane-api::pages::json_agent_test`, `agents::run::draft`).

- **`POST /api/v0/agents/{id}/test/messages`** (`write` share) `{message,
  source:"draft"|"published", session_id?}`. Ordinary agents require `draft`;
  organization member writers can also choose `published`. It opens a message
  against the selected source, claims the conversation as the public endpoint does (`agents::embed::claim`) and
  answers `202 {session_id, turn_id, source, agent_version}` at once; the turn runs
  in the background (`run::draft::start_private_turn`, then
  `embed::spawn_guarded` with `TurnWork::Private`) through the ordinary run path,
  as the agent's principal: its grants, gates, binds and budgets apply and its
  tools really run. No `session_id` starts a conversation; one continues it.
  Failures, all before anything runs: `404 agent_unavailable`, `404
  unknown_session`, `409 decision_pending` (the conversation waits for a
  decision), `409 turn_in_progress`, `422 agent_not_runnable` (a bad spec,
  including a strict scope without a refusal), `503 agent_no_model` (the spec
  names no model and the gateway has no default chat model), `422
  agent_model_not_granted`.
- **`GET /api/v0/agents/{id}/test/{session}/events`** (`write` share) is the
  test conversation's `chat_json` stream, exactly a person's chat's
  (`chat::json_api::{live_stream, quiet_stream}`): a `snapshot`, then the
  running turn's deltas, tool calls and `turn_finalized` — or `suspended` when
  it pauses — or `idle`. Only the admitted private source/version of this agent streams here; any
  other id is `404`. Organization history requires current member Write access.
- **`GET /api/v0/agents/{id}/test/{session}/turns/{turn}/debug`** (`write`
  share) is that turn's `{turn_id, source, agent_version, debug, handoff}`, read once
  the turn has settled: it waits up to 15 s for the conversation's claim to
  drop (the output filter rules after the row is final), else `409
  turn_in_progress`. `handoff` is what the turn hands to a person while it
  waits on one (the pause's `run_context.handoff`), else `null`.
- **A pause** in a test conversation is answered by the manager through the
  staff resume route, a `secure_input` included (the manager plays the
  visitor); draft pauses stay inline. Published private organization pauses also reach
  the current member-writer Inbox ([`agent-hil.md`](agent-hil.md#answering)).

**The debug view** is for managers only and is built from the conversation's
state now and the turn's activity events (`run::draft::collect_turn_debug`:
the events written from the turn's start until the next answer's, a pause and
its resume included), never from anything a visitor can reach, and never
stored anywhere else:
- `slots`: per declared slot `{slot, status: set|missing|invalid, value,
  provenance, set_at, set_by, reason?}`. Unlike the model's view, `value` is
  shown for verifier and host slots too.
- `routes`: per route `{route, description?, open, missing: [Unmet]}`, judged
  on the state after the turn.
- `routing`: this turn's `route_decision` (every gate, the route picked).
- `sub_agents`: this turn's `sub_agent_dispatched`, replaced by the matching
  `sub_agent_finished` (with `outcome`) once it ended — paired by the child's
  `turn_id`, or an A2A dispatch's `dispatch_id`.
- `loops`: the `loop_iteration` and `loop_finished` events in order, each with
  its `event`.
- `tool_calls`: this turn's `tool_call` decisions (`allowed`/`denied` and the
  policy).
- `scope`: a strict scope's verdict `{verdict, topics, error?}`.

**How private sources run.** The synchronous evaluator `run_draft_turn` loads
`SpecSource::Draft(spec)` and refuses organization execution. The authenticated
`start_private_turn` admits Draft or organization Published using the common
profile loader behind its explicit private purpose. New Published uses the
actual live pointer, and continuation loads its captured version. Both open
their session and use the same `drive_opened` as a visitor's turn
([`agent-runs.md`](agent-runs.md#entry-points)). Draft is recorded as version `0`
(`DRAFT_VERSION`). A sub-agent the draft dispatches to runs its own live
version. The `embed` integration test publishes v1, edits the draft, and shows
the test chat running the draft while a visitor message gets v1. Test
conversations are kept and swept like any other; they have no history list in
the builder.

## Resources a manager may grant

**`GET /api/v0/agent-resources`** (`aiplane-api::pages::json_agent_resources`)
lists what the calling manager holds and can therefore grant: `{models:
{chat, transcription, speech}, defaults, items}`. It applies the same
predicates as the grant route's cap, so the builder's pickers never offer what
`POST …/grants` would refuse with `grant_exceeds_manager`.

- `models.<kind>` is `model_choices::offered` for the manager
  ([`agents.md`](agents.md#models)); each speech model lists its `voices`.
  `defaults.<kind>` is the gateway default an unset key runs on.
- `items` are the tools, connectors, skills and knowledge bases, each the row
  the chat capability picker shows for the same resource — built by the same
  constructors (`tool_toggles::CapabilityEntry::{tool, connector, skill,
  collection}`), so `key`, `kind`, `title`, `description`, `group` and `icon`
  are identical: a tool reads as its catalog entry, everything else as its
  admin wrote it, and an empty description stays empty. Each item adds `grant:
  {kind, refs}` (a catalog entry standing for several tool ids — memory, a
  typst template, ComfyUI — grants all of them), `tools` (the ids it puts into
  `main.tools`), and `editable` / `config_url` — only where the viewer can fix
  the resource itself: a tool's copy lives in the catalog, so no tool links; an
  admin is pointed at a connector's `/admin/connectors/<key>/edit`,
  `/admin/skills`, or a knowledge base's `/rag/<id>/edit`. Knowledge bases
  (`kind` `rag_collection`, group `knowledge-base`) are offered only here; the
  chat picker searches knowledge through its tools. What each surface *lists*
  stays its own: the chat what the person may use, agents what the manager may
  grant.

## Evaluation

Stored test cases per agent, run against the draft or a published version and
judged on more than the final answer. Rows in
`aiplane-agents::db::agent_tests` (`agent_test_cases`, `agent_test_runs`,
`agent_test_results`); the logic in `aiplane-runtime::agents::eval` (and
`eval_judge` for the rubric).

- **Case.** `{name, script, expect, rubric?}`; the name is unique per agent. A
  case is validated when stored (`422 invalid_test_case` with `issues[{path,
  message}]`): a case that checks nothing, or names an unknown key, is refused.
- **Script.** An ordered list of steps: `{"say": "<visitor message>"}` or
  `{"write": {"slot", "value", "writer"}}`, with `writer` either `host` or
  `verifier:<id>`. A write goes through `write_trusted` with a `TrustedWriter`
  built from that text, so the slot's `set_by` and its type still apply; a
  refused write fails the case with the reason. It is the only place text
  becomes a trusted writer, and only the test runner reaches it: no public or
  visitor path accepts a script. The model's own `llm` writes cannot be
  scripted. The first step must be a `say`, because the conversation only
  exists once the visitor has spoken. A turn that suspends (an approval, a
  secure input) before the last step stops the script: a script cannot answer
  it.
- **Expectations** (`expect`, all deterministic; an omitted key is not
  checked):

  | Key | Meaning |
  |---|---|
  | `gates.<route>` | `{open: bool, missing?: [slot]}`: the gate after the last step; for a closed gate, these slots must still be among what it misses |
  | `route` | the route `forward_request` picked last (`"billing"`), or `null` for none |
  | `sub_agents` | `{called?: [route], not_called?: [route]}`, by route name |
  | `bound` | `[{route, name, equals}]`: the route was dispatched, and its `bind` resolves `name` to `equals` |
  | `tools` | `{called?: [tool], not_called?: [tool]}`: allowed `tool_call` decisions of the run, `set_<slot>` and `forward_request` included |
  | `answer` | `{contains?: [text], not_contains?: [text]}`, case-insensitive, on the last answer as delivered (after the output filter) |
  | `filter` | `passed`, `withheld` or `redacted`: what the output filter did to the last answer |
  | `finished` | `true`: the last turn ended with an answer; `false`: it did not |

  `bound` reads the values from the final state with the same resolver
  dispatch uses, so a state change after the dispatch can differ from what was
  passed. `tools` and `sub_agents` read the events of the whole conversation,
  nested sub-agents included.
- **Rubric.** Optional free text per case. After the deterministic checks, one
  side call (`server::side_call`, a usage row of the agent's, under its budget,
  an `llm_exchange` with `purpose: rubric_judge`) on the agent's main model, as
  its principal, grades the visitor messages and the agent's answers (nothing
  else: no slot values, no tool results) as `{passed, reason}`. It is reported
  as `report.rubric` (`verdict`: `passed`, `failed`, `error`, `skipped`) and
  counted apart in the run's `rubric`. It never changes a case's `passed`, a
  run's `green` or the publish guard.
- **Goal-Plan-Action report.** Each case yields `{passed, error, goal, plan,
  action, rubric, turns, debug}`; each section is `{passed, checks: [{check,
  passed, expected, actual, message}]}`.
  - *Goal*: `finished`, `answer.*`, `filter` — did the conversation end as
    meant.
  - *Plan*: `gates.*`, `route` — did the gates hold and the router choose
    right.
  - *Action*: `sub_agents.*`, `tools.*`, `bound` — were the right things
    called.

  `passed` holds when the script ran through and every check in the three
  sections holds. `turns` carries each message with its status and answer;
  `debug` is the test chat's debug view after the last step.
- **Running.** Each case is its own conversation through `run_draft_turn`, the
  test chat's door: version 0, the agent's grants, gates, binds and budgets,
  its tools really running. A version is tested by passing its stored spec as
  the draft, so no second execution path exists and nothing about "live" is
  overridden. A sub-agent a case dispatches to runs its live version. Cases run
  one after the other in the order they were created, and the request returns
  when the suite is done.
- **Test data.** Version-0 conversations are what analytics leave out and what
  retention sweeps like any conversation; a stored result keeps its report
  after the conversation is swept (`session_id` carries no foreign key).

**API** (`aiplane-api::pages::json_agent_tests`; `read` lists, `write` writes
and runs, admins hold both):

| Method | Path | Share | Purpose |
|---|---|---|---|
| GET | `/api/v0/agents/{id}/tests` | read | `{cases, latest_draft_run, latest_draft_run_current}` |
| POST | `/api/v0/agents/{id}/tests` | write | create `{name, script, expect, rubric?}`; 201, 409 on a taken name |
| PUT | `/api/v0/agents/{id}/tests/{case}` | write | replace a case |
| DELETE | `/api/v0/agents/{id}/tests/{case}` | write | delete a case; 204 |
| POST | `/api/v0/agents/{id}/tests/run` | write | `{source: "draft" \| "version:N"}`: run every case; 201 with the stored run and its results. 400 without cases or with another `source`, 404 for an unknown version |
| GET | `/api/v0/agents/{id}/test-runs` | read | the newest 50 runs, without results |
| GET | `/api/v0/agents/{id}/test-runs/{run}` | read | one run with a result per case |

A run is `{id, source, version, started_by, started_at, finished_at, passed,
failed, green, rubric?, results?}`; `green` means no failed case and at least
one passed. `passed` and `failed` count the deterministic result only.

**Publish guard.** `publish.require_passing_tests: true` makes `POST
…/publish` answer `422 agent_tests_failing` unless the newest draft run is
green **for the draft and the suite as they are now**: each run stores a hash
of the spec it ran (`spec_hash`) and of the cases (`suite_hash`), so editing
either makes the last run stale and the message says to run the suite again.
The spec hash (`agent_tests::spec_hash`) counts every sealed credential
(`*_sealed`) as a fixed marker: the at-rest key rotation re-seals them with new
ciphertext, which is no change the suite tests, so it leaves a green run
current. With failing cases, `error.failing` lists `[{case_id, case_name,
problems}]` with the failed checks in words; with no cases or no matching run
it is empty. Rolling back (`/live`) is not guarded: it publishes nothing new.

Tests: `crates/aiplane/tests/it/agent_evaluation.rs` (a passing suite, failing
gate and route expectations, a trusted write with a bound value, a refused
trusted write, the filter outcome, a version run, the publish guard through its
whole cycle, the rubric reported apart, validation, the share rules, analytics
unchanged by a run), unit tests in `eval.rs`.

## Setup assistant

The plain-language setup ([`ui.md`](ui.md#agent-setup)) reads and writes the
same spec as the advanced editor. What it relies on, server side:

- **Model.** One model picker per model key over the models the manager may
  grant ([`agents.md`](agents.md#models)), with "Default (<model>)" for the
  gateway's default. Choosing a model grants it through the ordinary grant
  route, so the manager's cap applies unchanged.
- **Templates.** `web/src/lib/agent-templates.json`: five starter drafts (FAQ,
  customer support with an e-mail-code identity check and a fallback to a
  person, lead qualification, internal helper, blank). A string `@<key>` is a
  catalog message.
  `tests/it/agent_test_chat.rs::every_setup_template_is_a_valid_draft_in_every_language`
  creates every template in all six languages through `POST /api/v0/agents`,
  so the validator accepts each as a draft.
- **Slot `order`.** The details step writes each row's position into the
  slot's `order` and sorts by it ([`agent-spec.md`](agent-spec.md#state)).
- **Reserved names.** The verifier `identity`, the slots `verified`, `topic`
  and `request`, and the route `fallback` belong to the setup. A route is the
  setup's when it has exactly the hand-off shape; anything else under those
  names, and every other route, slot or verifier, is shown as "set up in the
  advanced editor" and kept as it is.
- What the model reads — tone and language lines, the hand-off task, the
  descriptions of `topic` and `request` — is English, like the system message.

## Prompt assistant

From a manager's scenario (typed or spoken) or a template, one model call
proposes a value for every setup step plus test cases; another improves one
text. The setup shows the proposal per step and applies what the manager
accepts. Code: `aiplane-runtime::agents::assist`; handlers
`aiplane-api::pages::json_agent_assist`.

Shared mechanisms it uses: `server::side_call::ask_json` for the structured
call, the spec validator (`spec::validate`, on the draft as it would be),
`eval::parse_case` for the proposed tests, the grant cap's predicates
(`json_agent_resources::grantable_tools` / `grantable_collections`, the lists
`GET /api/v0/agent-resources` serves), the rate primitive (`rates::record_now`,
counter scope `manager`), the spend limits (checked by the side call),
`read_json_capped`, and the activity log's runtime door
(`agents::audit::record_event`).

**It writes nothing.** Neither endpoint touches the draft, the grants, the
versions or the test cases. A scenario that talks the model into proposing
anything at all — "publish this", "grant run_in_sandbox", "hand everything to
agent X" — yields at most an offer the manager sees, filtered as below; the UI
applies an accepted step through the ordinary draft, grant and test routes,
which check it again (the grant route with its cap). The scenario is sent as a
JSON field of the user message, never as instructions, and the system message
says to ignore instructions in it.

**API** (`can_manage_agents` and a `write` share, admins hold one):

| Method | Path | Body | Answer |
|---|---|---|---|
| POST | `/api/v0/agents/{id}/assist/suggest` | `{scenario, template?, current_draft?, model?}` | `{steps, dropped, model, usage}` |
| POST | `/api/v0/agents/{id}/assist/improve` | `{field: task\|tone\|refusal, text, model?}` | `{field, suggestion, why, model, usage}` |

`steps` (each `null` or empty when nothing is offered):
- `task: {orchestration}` — `main.instructions.orchestration`;
- `tone: {chips, language, response}` — `main.instructions.response` as the
  task-and-tone step holds it: `chips` are the step's tone ids (`friendly`,
  `factual`, `casual`, `brief`, `detailed`, `formal`, `informal`; the schema
  enumerates them and the review drops any other word), `language` is
  `visitor`, an answer-language code or `null`, and `response` is only the
  further text no chip or language stands for. The lines each chip and language
  stand for live in `assist::tone` (a port of `TONE_LINES` / `responseText`,
  pinned against `agent-setup.ts` by a test), so an architect's tone is written
  exactly as the step reads it back;
- `scope: {topics, refusal, strict}` — `scope` (an existing
  `classifier_model` is kept);
- `abilities: [{id, name, why}]` — tools for `main.tools`, which the UI grants
  when applied; `name` is the title the ability card shows. Never
  `rag_search` / `rag_list_collections`: knowledge search without a collection
  finds nothing, so it is offered as knowledge (dropped with a reason if the
  model names it anyway);
- `knowledge: [{id, name, why}]` — knowledge bases (RAG collections) the
  manager may read and so grant, chosen by name and the admin's description
  (the model is told a base about another product does not fit, and to name the
  subject under `missing_knowledge` when in doubt); applying one does what its
  knowledge card does: the collection and `rag_search` granted, the search bound
  to it as a constant when it is the only one, `rag_list_collections` added
  when there are several (`review::set_knowledge`, a port of `setKnowledge`).
  Dropped when the manager may not grant `rag_search`;
- `missing_knowledge: [subject]` — what the agent must know about that none of
  those knowledge bases covers; the UI says an admin has to add one;
- `slots: [{name, label, type, def}]` — `state.<name> = def`. `type` is a
  friendly kind: `text` (string, ≤ 200, the setup's shape), `long_text`
  (string, ≤ 2000), `email`, `number`, `whole_number` (integer), `yes_no`
  (boolean), `choice` (enum of the proposed values); `set_by: [llm]`,
  `description` = `label`;
- `identity: {method, why}` — `none`, `website_login`, `email_code` or
  `lookup`; a recommendation only, since each needs a connector or a key the
  identity step asks for;
- `handoffs: [{name, topic, target, target_name, details, identity, route}]` —
  a rule of the hand-off step, `routes.<name> = route` in the shape
  `assist::handoffs::write` gives it. The model picks a `condition`: `always`,
  `details` (wait until every slot of the details step is set — a qualified
  lead) or `identity`. A rule about a topic the draft already hands off is left
  out. `identity` on a draft with no identity check is kept when the same offer
  recommends one (other than `none`): applying the hand-offs sets up that check
  first (`applySuggestedRules`), so the rule is written gated — `route` carries
  no identity leaf until then. With no check recommended either, the condition
  is left out with a note;
- `tests: [{name, kind, script, expect}]` — the body `POST …/tests` takes, at
  most 6. Every case expects `finished: true`; an `out_of_scope` case expects
  the draft's refusal in the answer (and is dropped when there is none yet).

`dropped: [{step, item?, reason}]` lists what was left out and why, in words.

**What may be offered.** The model is given — and its schema enumerates —
exactly the tools this manager holds and may grant (ids with their catalog
titles, knowledge search aside), the knowledge bases they may read (by name),
and the agents shared with them (not the agent itself) plus `human` as
targets. Because a backend may ignore the schema, the review checks again: an
ability or a target outside those lists is dropped, never offered.

**Validation per piece.** Pieces are applied in step order to the draft the
manager is editing (`current_draft`, else the stored draft), each checked with
`spec::validate` at the `Draft` stage against the agent's grants as stored now
plus the tools and collections offered so far (applying an ability or a
knowledge base grants it). A piece is kept when it adds no issue the draft did
not already have — so a draft that is invalid already does not sink the
proposal — and dropped with the issues it would add. Later pieces build on kept
earlier ones: a hand-off waiting for the details gates on the slots that were
kept. A slot or route name the draft already has is kept as it is, not
replaced. Names are made spec identifiers (`Order Number` → `order_number`). A
step whose JSON does not read is dropped whole; the others stand.

**Model.** The request's `model` when it is one of the chat models the manager
may use (`403 assist_model_not_allowed` otherwise), else the draft's
`main.model` when they may use it, else the first of their chat models — the
gateway's default chat model when they may use it (`assist::choose_model` over
`server::model_choices::offered`); `503 assist_no_model` when they may use
none. The manager's access decides, not the agent's grants: it is the
manager's call. An automatic-route alias is resolved by its selector, as in the
chat.

**Recording.** *Chosen:* a manager's call, not an agent run.
- A usage row of the manager's (`principal_kind = user`, `source = chat`, like
  the rest of the session UI), under the manager's own spend limits (`429
  rate_limit_exceeded` with `Retry-After` when over).
- In no conversation chain. The agent's own chain gets one `assist_suggested`
  event per call, actor = the manager: `action` (`suggest`/`improve`), the
  `scenario` and `template` (or the improved `field` and `text`), `model`,
  `usage` (token counts), `latency_ms`, `offered` (the step kinds), `dropped`,
  and `error` for a failed call. *Chosen to keep the scenario:* it is the
  manager's own input, and without it the event cannot explain what was
  proposed. The proposal itself is not kept (it is in the answer, and what is
  applied is recorded by the routes that apply it). The event is written for a
  failed model call too; when it cannot be written the call answers `503
  activity_log_unavailable`.

**Limits.** `ASSIST_RATE` = 30 calls per manager per agent per hour, suggest
and improve together, on the agent's `rate_events` (counter
`assist:<user id>`); `429 assist_rate_limited` with `Retry-After`. The rate is
per agent because `rate_events` is keyed by the principal; the spend limits
bound a manager across agents. Scenario and improved text at most 8000
characters, template at most 200, body at most 512 KiB (`400
invalid_assist_input`, 413). Model timeouts 120 s (suggest) and 60 s
(improve); a failed or unreadable answer is `502 assist_model_failed`.

Tests: `agents/assist/review/tests.rs` (a good proposal applied to the draft
passes `spec::check` at draft and publish stage; an ungrantable ability never
offered; tone chips only by the step's ids; knowledge by knowledge base, never
as the search tools, and wired like its card; an invalid slot dropped with the
validator's reason and a details hand-off gating on the slots that were kept;
targets limited to shared agents and people; unreadable steps dropped whole;
test-case rules; existing names kept; the schema's enums);
`tests/it/agent_assist.rs` (the endpoint end to end on a scripted model:
checked steps, every offered test case accepted by the tests route, nothing
written, the event and the usage row; knowledge bases offered by name and
refused without knowledge search; a prompt-injection scenario; improve; the
rate refusing with `Retry-After` and per manager; bad input and missing shares
refused before the model is asked; a failed model call recorded and answered
`502`).

## Agent architect

A conversation in which a built-in assistant plans an agent with a manager —
asks questions, proposes a structure, explains trade-offs — and creates or
changes the draft as it goes. The UI is in [`ui.md`](ui.md#agent-architect).

**Chosen: a persona of the person's chat, not an agent.** The architect is not
a system principal and holds no grants. Its conversation is an ordinary chat
of the signed-in person's (`chat_sessions.user_id`), marked in
`agent_architect_sessions` (session, person, the agent it plans), and every
turn runs on the person chat driver as `TurnPolicy::Persona`
(`aiplane_runtime::persona::ChatPersona`): the person's models, budget, usage
and history, but the architect's fixed system prompt and only its tools,
offered every round. The person's own chat tools are neither offered nor run
(a call to one is refused as an unknown tool). The prompt is English and tells
the model to answer in the person's language.

**Tools** (`aiplane-api::pages::architect::tools`). Each acts as the person
through the function the matching route uses, so the share, the validator and
the grant cap decide exactly as for a click in the setup UI, and a refusal
reaches the model — and the chat — as the route's own message and code. Each
checks `can_manage_agents` again when it runs.

| Tool | Does | Through |
|---|---|---|
| `list_agents` | the agents shared with the person | `visible_agents` (as `GET /api/v0/agents`) |
| `read_agent(agent_id)` | draft, grants, what blocks publishing | `agent_by_id` (`read` share), `SpecWorld`, `publish_issues` |
| `list_grantable` | models by kind, the gateway's default models, and `items` (tools, connectors, skills, collections with their own titles, descriptions and grant refs) | `resources_for` (as `GET /api/v0/agent-resources`) |
| `propose_setup(agent_id, scenario, template?)` | the prompt assistant's proposal; writes nothing | `suggest_for` (as `…/assist/suggest`: its rate, usage row and `assist_suggested` event) |
| `create_agent_draft(display, id?, description?)` | a new agent, unpublished; the id is derived from the name like the create dialog's `agentIdFromName`; its `main.model` is left unset, so it runs on the gateway's default chat model, which is granted (capped) when the person may grant it; the answer names it as `model` | `create_agent` (as `POST /api/v0/agents`) |
| `update_agent_draft(agent_id, changes)` | changes the draft step by step | `assist::apply_changes`, then `add_capped_grant` per needed grant, then `save_draft` |
| `run_test_turn(agent_id, message, conversation_id?)` | one test-chat turn of the draft, run to its end inside the architect's turn | `draft_test_turn` (`run_draft_turn`, then the turn's debug view) |

There is **no publish tool**: the prompt tells the model to send the person to
the setup page (`setup_url`), where Publish stays a click. An agent may be
named by its id or its name.

**`changes`** (`assist::changes_schema`): `display`, `model`, `task`, `tone`,
`scope`, `abilities` and `slots` in the prompt assistant's step shape, plus
`knowledge: [{name, why}]`, `handoffs: [{topic, target, details?, identity?}]`
and `fallback_to_person`. `apply_changes` runs the same `Reviewer` as the
prompt assistant: each piece is applied to the stored draft and kept when it
adds no validator issue, else dropped with the reason. An ability is offered
only when it is one of the person's grantable tools, and `model` only when it
is a chat model the person may grant — both are then granted through the
capped grant route before the draft is saved, so a grant the person lacks is
never made. When nothing at all is applied the call fails with the reasons, so
the chat shows it as an error.

*Written in the setup's shapes.* What the architect writes must read in the
setup as what it is, not as "set up in the advanced editor":
- a slot gets the setup's friendly-kind shape (`text` is ≤ 200, as
  `SLOT_SHAPES`) and the next `order`; a hand-off waiting for every detail is
  regated on the new set (`handoffs::with_details`, the port of `withDetails`,
  which the details step and the identity step's added slots go through as
  well);
- hand-offs are the hand-off step's rules: `assist::handoffs` is a port of
  `readHandoffs` / `writeHandoffs` / `deriveBind` from
  `web/src/lib/agent-setup.ts` (routes `when: {all: [topic eq, request set,
  (one `set` leaf per detail slot — read as a rule only when the leaves name
  exactly the detail slots, in any order; a route on some of them is custom and
  kept verbatim), (verified provenance)]}`, `task` `Request about {topic}:
  {request}`, the `topic` enum and `request` slots, `router.order`, the
  `fallback` route). A rule whose topic exists replaces it; other routes stay
  as they are. The bind comes from the specialist's live spec; `identity` is
  honoured only when the agent has an identity check (an architect cannot set
  one up, so unlike the prompt assistant's offer a recommended method does not
  count), `details` only when it collects details (otherwise kept without it,
  with a note). `web/src/lib/fixtures/architect-draft.json` is a draft written
  by `apply_changes` (pinned in `review/tests.rs`) that `agent-setup.test.ts`
  reads back as rules and friendly slots, and writes back unchanged.

Identity checks need a connector or a key only the setup's identity step asks
for, and test cases are saved from its last step; both are left to the UI.

**Starting.** `POST /api/v0/agent-architect` `{agent_id?, title, fresh?}` →
`{session_id, model, agent_id, resumed}` needs `can_manage_agents` (403
otherwise) and a `write` share on `agent_id`. It reopens the person's newest
architect conversation about that agent (or about no agent yet) unless
`fresh`; otherwise it creates one titled `title` (the client sends "Agent
architect: <name>" in the person's language). `model` is what to send messages
with: the prompt assistant's choice (`assist::choose_model`), `503
architect_no_model` when the person may use none. Messages then go through the
ordinary `POST /api/v0/chat/sessions/{id}/messages` and stream on its events
endpoint; `spawn_assistant_worker` looks the session up and builds the persona
for it.

**Chosen: the conversations stay in the chat history.** They are the person's
chats, titled "Agent architect: …", and are not hidden from the chat list:
hiding would need a filter in the session list and the sidebar for no safety
gain, and opened from `/chat` they still run as the architect, since the
persona belongs to the session, not to the page.

Tests: `agents/assist/review/tests.rs` (changes applied with their grants; a
model or tool the person may not grant dropped); `openai_driver/resume.rs`
(`a_personas_turn_offers_only_its_tools_and_refuses_the_persons`);
`persona.rs`; `db/architect_sessions.rs`; `pages/architect/tools.rs` (the id
from a name, no publish tool); `tests/it/agent_architect.rs` (a scripted
conversation lists, creates, proposes, updates, is refused an ability the
person lacks, tests the draft and cannot publish, every call recorded; the
offered tools; undo through `draft/restore`; reopening and `fresh`; 403 for a
non-manager; an undo revoking the change's tool but keeping the model the
restored draft runs on, logged; an undo keeping a grant the live version
uses); `web/src/lib/architect.test.ts`.

## Draft revisions

Every draft save (the UI's and the architect's) keeps the draft it replaced in
`agent_draft_revisions` (the newest `MAX_DRAFT_REVISIONS` = 50 per agent; an
unchanged save keeps none) and answers with that `revision`.
`update_agent_draft` returns it, and the architect window's *Undo* calls
`POST /api/v0/agents/{id}/draft/restore {revision}` → `{draft_spec,
live_version, revision, revoked}`, which validates and saves it like any draft
(so the undo is itself undoable).

A revision also keeps the grants its change made
(`agent_draft_revisions.granted`, filled by the architect's
`update_agent_draft` and `create_agent_draft`). The undo revokes those after
the save unless the restored draft or the live version still uses one — "uses"
is asked of the validator (`uses_grant`: the spec has more issues without the
grant), not a list of fields. The activity log has the draft event with
`restored` and `revoking`, and a `grant_removed` per revocation; the answer
lists them as `revoked`. Tests: `db/agents.rs` (revisions kept, capped,
unchanged saves skipped), `tests/it/agent_architect.rs`.
