# The agent spec

An agent's spec is one JSON document: `agents.draft_spec` while it is edited,
`agent_versions.spec` once published ([`agents.md`](agents.md#agent-definition)).
The builder UI and the API speak the same shape. This file covers its
layout, how it is validated and typed, the conversation state it declares
and the gates over that state. How a run uses it is
[`agent-runs.md`](agent-runs.md).

## Layout

YAML here only for readability; the stored form is JSON.

```yaml
profile: { display: "croit Support", avatar: null, color: "#4f46e5" }
scope:                                  # all optional
  topics: [croit products, Ceph storage]
  refusal: "I can only help with croit products and Ceph storage."
  strict: true                          # a topic guard refuses everything else
  classifier_model: small-fast          # the guard's model; default the main model; must be granted
main:
  model: qwen3                          # must be granted; unset = the gateway's default chat model
  instructions:
    orchestration: "Collect name, email and issue before forwarding …"
    response:      "Friendly, short, in the visitor's language …"
  tools: [rag_search]                   # must be granted; mcp__<key>__<tool> needs grants[connector]
  skills: [brand-voice]                 # must be granted
  tool_resources:
    rag_search: { bind: { collection: { const: "produktdoku" } } }
    mcp__erp__refund: { permission: always_ask, approval_timeout: 2h }
  budget: { rounds: 12, seconds: 60, tokens: 40000 }
state:                                  # slot name -> definition
  name:     { type: string, max_length: 120, set_by: [llm] }
  email:    { type: email,  set_by: [llm] }
  issue:    { type: enum, values: [billing, technical, sales], set_by: [llm] }
  verified: { type: subject, set_by: ["verifier:otp", host] }
  issue_summary: { type: string, max_length: 2000, set_by: [llm], description: "What it is about", order: 3 }
verifiers:                              # agent-visitors.md → Identity verifiers
  otp: { kind: mcp_code, connector: erp, email_slot: email, writes: { verified: result } }
router: { kind: rules, order: [billing] } # or { kind: classifier, model: small-fast }
routes:
  billing:
    description: "Invoices and payments"
    when: { all: [ { slot: issue, eq: billing },
                   { slot: verified, provenance: "verifier:otp", max_age: 15m } ] }
    agent: 5b1c…                        # the agent id (system_principals.id) of another agent
    task: "Invoice question from customer {verified.customer_id}: {issue_summary}"
    bind: { customer: state.verified.customer_id }   # passed to the sub-agent as route.customer
  human:
    when: { slot: issue, set: true }
    human: { notify: [push, slack], inbox: support, timeout: 30m }
  # further targets: a2a (agent-a2a.md), loop (agent-runs.md → Loop routes)
finish: { schema: { type: object, required: [answer], properties: {
            answer: { type: string }, facts: { type: array, items: { type: string } } } } }
publish:
  origins: ["https://www.example.com"]
  idle_ttl: 30m
  retention_days: 30                    # default 30
  audit_retention_days: 365             # default 365, at least retention_days
  rate_limits:                          # defaults: visitor 20 per 10m, ip 60 per 10m
    visitor: { max: 20, per: 10m }
    ip:      { max: 60, per: 10m }
  budget: { monthly_cost: 50, monthly_tokens: 2000000 }   # no default
  output_filter:
    patterns: { invoice: "RE-\\d{6}", customer: "K-\\d{5}" }
    action: withhold                    # withhold (default) | redact
  require_passing_tests: false          # true: publish needs a green test run of this draft
  a2a: { enabled: true }                # serve over A2A; skills optional (agent-a2a.md)
  voice: { input: true, output: true }  # agent-visitors.md → Voice
```

A sub-agent's spec uses the same layout. Its input is the rendered `task`,
and it ends in `finish`. Which part of the spec each subsystem reads:

| Part | Read by |
|---|---|
| `profile`, `main`, `state`, `router`, `routes`, `finish` | the run ([`agent-runs.md`](agent-runs.md)) |
| `scope` | the system message and the topic guard ([`agent-runs.md`](agent-runs.md#topic-guard)) |
| `verifiers` | [`agent-visitors.md`](agent-visitors.md#identity-verifiers) |
| `tool_resources.<tool>.permission`, `human` routes | [`agent-hil.md`](agent-hil.md) |
| `publish.origins`, `idle_ttl`, `rate_limits`, `budget`, `retention_days`, `voice` | [`agent-visitors.md`](agent-visitors.md) |
| `publish.output_filter` | [`agent-runs.md`](agent-runs.md#output-filter) |
| `publish.audit_retention_days` | [`agent-activity-log.md`](agent-activity-log.md#retention) |
| `publish.a2a`, `a2a` routes | [`agent-a2a.md`](agent-a2a.md) |
| `publish.require_passing_tests` | [`agent-builder.md`](agent-builder.md#evaluation) |

### Tool resources

`tool_resources.<tool>` holds:

- **`bind`**: the tool's parameters the gateway fills in, mapped explicitly
  `{<tool parameter>: <source>}`. They are removed from the schema the model
  sees, and the model's value for them is overwritten. A source is one of:
  - `"state.<slot>[.<field>]"`, a slot of this agent's state the model cannot
    write (`"state.verified.customer_id"`);
  - `"route.<name>"`, a value the route that dispatched this agent passes;
  - `{const: …}`, a fixed value.

  A route's own `bind` is `{<name>: "state.…" | {const: …}}`: the values it
  resolves from its caller's state and passes down as `route.<name>`. A
  sub-agent's tools bind `route.<name>` or `const`, as in `tool_resources: {
  mcp__erp__invoices: { bind: { customer_id: "route.customer" } } }`.

  A **subject parameter** is a parameter name that some tool of the agent
  binds from `state.` or `route.`. Any other tool of the agent that declares a
  parameter of that name must bind it too; otherwise it is not offered, and a
  call to it is refused. A `const` bind fixes a setting and makes no name a
  subject. How binds run: [`agent-runs.md`](agent-runs.md#bound-arguments).
- **`permission`**: `always_allow` or `always_ask`, which suspends the call for
  staff approval. Without one, a tool its server marks destructive and not
  read-only asks. `approval_timeout` (default `1h`) bounds the wait; nobody
  approving is a deny ([`agent-hil.md`](agent-hil.md#per-tool-approval)).

## Validation

`aiplane-runtime::agents::spec`. The validator walks the JSON and returns
every problem as `{path, message}` rather than stopping at the first one; the
API answers `422 invalid_agent_spec` with all of them, and the builder shows
each under the field its path names. It runs on every save and create, and
again on publish against the grants as they are then. What it checks the
spec against is a `SpecContext`, loaded by `SpecWorld`: the principal's
grants, which agents exist and are live, the live specs of every agent (the
sub-agent graph), the model defaults, the voices each speech model offers,
and `$AIPLANE_ALLOW_PRIVATE_NETWORKS`.

**On every save (`Stage::Draft`)** — shape and references:
- Unknown keys at any level are errors, never ignored: a misspelt
  `tool_resource` must not silently leave a tool unbound. Types, enums,
  durations (`30s`/`15m`/`2h`/`30d`), exact origins and regexes are checked;
  the `finish` schema goes through `FinishContract::new`, the run-time
  validator itself.
- Models, tools, MCP tools (via their connector), skills and verifier
  connectors must be granted to this agent's principal. The spec cannot widen
  what the principal holds; it can only pick from it.
- Sub-agents must be existing agent ids, and not the agent itself. Gate leaves,
  templates and `set_by: verifier:<id>` must name declared slots and
  verifiers. Every `{slot}` in a template must exist in `state`.
- Every `state.` bind source must be a slot whose `set_by` lacks `llm`; a
  route's `bind` cannot use `route.`.
- Slots: each constraint must apply to the slot's type (`pattern` on an
  `integer` is an error, not silently unenforced), a `subject` slot's `schema`
  must be one the run-time validator enforces, and the model may not write a
  `subject`.
- Gates type-check against the slots ([Gates](#gates)), including the
  trusted-gate rule.
- The sub-agent graph, over the live specs of the agents routed to, is acyclic
  and at most 3 agents deep, this one included (`run_chain::MAX_DEPTH`). A
  route whose graph loops back, or nests deeper, is an issue at
  `routes.<r>.agent`.
- A route passes exactly the `route.<name>` values its live sub-agent binds,
  no more and no fewer.
- A `strict` scope lists at least one topic and has a non-blank `refusal`;
  its `classifier_model`, when set, must be granted. Checked on every save,
  because the test chat runs the draft.
- `publish.audit_retention_days` is at least `publish.retention_days`, so a
  conversation's log outlives the conversation.
- The parts each subsystem owns validate their own keys: verifiers
  (`spec/verifiers.rs`), route kinds (`spec/route_kinds.rs`), `publish.a2a`
  (`spec/a2a.rs`).

**On publish (`Stage::Publish`)** — everything above, plus what a run needs:
- instructions, and a live version for every routed sub-agent;
- a `finish.schema` on every routed sub-agent;
- a granted model for every model key left unset: `main.model` and each voice
  direction that is on run on the gateway's default of their kind
  (`SpecContext::model_defaults`, [`agents.md`](agents.md#models)), so that
  default must exist and be granted;
- what each verifier needs to run, and a `publish.voice.voice` the speech
  model offers.

`publish_issues` in `GET /api/v0/agents/{id}` is the publish-stage result for
the current draft.

Whether a tool *declares* a subject parameter is known only from its schema,
and an MCP tool's schema exists only once its connector is connected. The
validator therefore checks the route ↔ sub-agent contract (`route.` names,
the finish schema); the unbound-subject rule is enforced at run time against
the real schema, by withholding the tool.

**Secrets in a spec** are sealed on save. `POST /api/v0/agents` and
`PUT …/draft` validate a plain credential and replace it with its sealed form
(`<key>_sealed`, the at-rest `Crypto`): the host-JWT `secret` and an A2A
route's `token` / `client_secret`. One helper does it,
`spec::secrets::seal_spec_secrets`, from one list of where credentials live
(`SPEC_SECRETS`), so no draft, version, activity event or GET carries a
plaintext. A GET → PUT round trip keeps the sealed value. The activity log's copies are
hash-chained and left as they are.

## The typed spec

Runtime code never reads the spec's JSON. It reads `AgentSpec`
(`aiplane-runtime::agents::spec::model`): serde structs and enums for every
part above — `profile`, `scope`, `main` (with `tool_resources`, their `bind`,
`permission` and `approval_timeout`, and `budget`), `state` slots,
`verifiers` (tagged by `kind`), `router`, `routes` (each with exactly one
`RouteTarget`: `agent`, `human`, `a2a` or `loop`), `finish` and
`publish`.

- **Validation and typing are two steps over one JSON.** `spec::check` runs
  the path-reporting walk; only when it found nothing does it deserialize the
  same value into `AgentSpec`. The walk keeps the `{path, message}` errors
  (serde would stop at the first and point at a line and column nobody
  typed); the types keep the shape. `spec::validate` is `check` without the
  typed value, so every accepting validator test also proves the two agree: a
  spec the walk passes but the types refuse comes back as an issue at the
  root, which is a bug.
- **Unknown keys are refused twice.** Every type is `deny_unknown_fields`,
  like the walk, so a misspelt or renamed key can never deserialize into a
  silent default.
- **Defaults live in the types.** A field the spec may leave out is an
  `Option` (or an empty collection), and the one accessor that reads it
  applies the default: `Publish::{idle_ttl, visitor_rates, retention_days,
  allows_origin, a2a_enabled}`, `RunBudget::budget`,
  `ToolResource::approval_timeout`, `HumanSpec::{timeout, transcript}`,
  `LoopSpec::max_iterations`, `A2aRouteSpec::seconds`, the `McpCodeSpec` and
  `LookupSpec` accessors, `HostJwtSpec::max_lifetime`,
  `VoiceSpec::{input_model, output_model}` (the named model only for a
  direction that is on) and `Profile::color` (`#rrggbb`). The constants they
  apply stay with the subsystem that documents them.
- **What stays JSON**: a route's `when` (compiled by `gate::Cond` into
  `RouteGates`), `finish.schema` and a `subject` slot's `schema` (JSON schemas
  `FinishContract` enforces), and `state`, which `StateSchema` compiles from
  the same JSON because the walk needs it too. The architecture test
  `agent_spec_json_is_read_only_by_the_validator` keeps every other reader on
  the types.
- **Built once.** A published version is immutable, so its `AgentSpec`,
  `StateSchema`, `RouteGates` and `OutputFilter` are compiled once per
  `(agent, version)` into a `CompiledSpec` and shared (`agents::spec_cache`, at
  most 256, least recently used first); `CompiledSpec::agent()` hands out the
  spec. Which version is live is read fresh for a run, and trusted for 5 s on
  a visitor admission (`LIVE_TTL`). A draft run, the test-chat debug view and
  a test case compile the draft the same way.
- **A stored spec that does not read** (only a hand-edited row; every save
  and publish ran `check`) fails loudly rather than falling back field by
  field: it cannot run (`BadSpec`, "it does not read as an agent spec …"), the
  embed endpoint allows it no origin, a visitor admission applies the default
  rates, the retention sweep the default 30 days, and it is not served over
  A2A.

## State

A conversation's typed state: one row per slot.

```sql
CREATE TABLE agent_state (
    session_id  TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    slot        TEXT NOT NULL,
    value       TEXT NOT NULL,      -- JSON, already validated
    provenance  TEXT NOT NULL CHECK (provenance IN ('llm', 'host') OR provenance LIKE 'verifier:_%'),
    set_at      TEXT NOT NULL,
    PRIMARY KEY (session_id, slot)
) STRICT;
```

It references `chat_sessions(id)` only, so it holds for any conversation.
Storage is `aiplane-agents::db::agent_state` (`put`, `for_session`); it knows
neither types nor writers, and `put` writes a `state_written` activity event
on the write's own transaction ([`agent-activity-log.md`](agent-activity-log.md)).

**Slot types** (`aiplane-runtime::agents::state`). `StateSchema::from_spec`
reads `state` into `SlotDef`s, and `SlotDef::check` validates in code and says
what to fix:
- `string`: `min_length`, `max_length` (characters), `pattern`;
- `email`: `local@domain.tld`, no whitespace; `max_length`;
- `enum`: one of `values`;
- `integer`, `number`: `minimum`, `maximum`;
- `boolean`;
- `subject`: an object, checked against an optional `schema` by the finish
  contract's validator (`finish::validate`). A `subject` says whose data the
  agent acts on, so it may not list `llm`.

Every slot may carry `description` (its label: the debug view, the hand-off
context and the setup show it) and `order` (a whole number ≥ 0, its place in
the setup's list of details). Nothing at run time reads `order`; it exists
because a JSON object's key order does not survive a save: `serde_json`
without `preserve_order` keeps object keys sorted, and turning that feature on
workspace-wide would change every `to_string()` of a map (`agent_tests::spec_hash`
among them). Slots without `order` follow, by name.

**Who can write a slot.** Only the writers its `set_by` lists, through two
doors:
- `set_<slot>` (`agents::slot_tools`) always writes provenance `llm`. It
  exists only for slots whose `set_by` lists `llm` and takes exactly `{value}`
  (`additionalProperties: false`); any other key, such as `slot`,
  `provenance` or `session_id`, is refused by name. The session comes from
  `ToolContext.session_id`. An invalid value is `InvalidArgs` ending in "call
  set_<slot> again".
- `write_trusted(pool, schema, session, slot, value, TrustedWriter, now)` is
  the only way to store `verifier:<id>` or `host`. `TrustedWriter` has no
  conversion from a string or JSON, so a value parsed from a tool call cannot
  become one. Verifiers and the host-JWT path use it
  (`state::write_trusted_all` for several slots in one transaction), and so
  does an evaluation's scripted write — the one place text becomes a trusted
  writer, reachable only from the test runner
  ([`agent-builder.md`](agent-builder.md#evaluation)).

Both refuse a writer outside `set_by` before looking at the value, then refuse
an invalid value with the validator's message. A rewrite replaces the row,
provenance and `set_at` included. Writes take `now`, and `SlotTools::with_clock`
injects it, so tests fix `set_at` and every `max_age` gate on top of it.

**What the model sees.** `AgentState::load` judges each stored row against the
current schema: `set`, `missing`, or `invalid` (a writer the spec does not
allow, or a value that does not fit). `view()` returns `SlotView`s and
`render_view()` the system-message block. A value is shown only when its
provenance is `llm`; a trusted slot reads `set by verifier:otp`, and its
invalid reason never echoes the value. The manager's debug view shows every
value ([`agent-builder.md`](agent-builder.md#test-chat)).

## Gates

A route's `when` is a JSON condition tree, evaluated by a small hand-written
evaluator (`aiplane-runtime::agents::gate`).

*Chosen over a string language:* that would need a parser, error recovery,
quoting rules and a way to show its errors in a form-based builder. A JSON tree
needs none of that: the UI composes it from dropdowns, and validation is a type
check, not a parse. An expression crate (CEL and similar) would add a
dependency ([`dependencies.md`](dependencies.md)) for roughly 150 lines of
evaluator, and bring operators we would then have to prove harmless.

```text
Cond := { all: [Cond] } | { any: [Cond] } | { not: Cond }
      | { slot, set?, eq?, in?, provenance?, max_age? }     -- at least one check
           set: bool
           eq: Value            -- the slot's whole JSON value (a subject compares as an object)
           in: [Value]
           provenance: "llm" | "verifier:<id>" | "host"
           max_age: Duration    -- now - set_at <= d; a value stamped after now is not old
```

A leaf's checks are ANDed. `Cond::parse` reads a tree; the spec validator
reports shape problems with paths. There is no slot-to-slot comparison.

**Semantics.** A slot that is missing, `invalid` or not declared fails every
check on it, except a leaf that is only `set: false`, which holds. `all` holds
if every child does, `any` if one does, `not` if its child does not, so
`{not: …}` over a missing slot holds.

**Guarantees.**
- **Total:** evaluation never fails.
- **Deterministic:** the only input besides state is `now`, which is injected
  so tests fix it.
- **Explainable:** `evaluate(cond, GateInput { schema, state, now }) ->
  Vec<Unmet>` returns every unmet leaf in document order; empty means open.
  For an `any` with no holding child it returns every child's leaves. An
  `Unmet` is `{path, slot?, kind, …, message}` with `kind` one of `missing`,
  `invalid`, `must_be_unset`, `not_equal {expected}`, `not_in {expected}`,
  `wrong_provenance {required, actual}`, `too_old {max_age}`, `excluded` (a
  `not` whose child holds), `unknown_route`, `denied`. The `message` tells
  the model what to do — `call set_email`, or `it is set by verifier:otp or
  host, not by you` — and never contains a value the model did not write;
  `expected` comes from the spec. This is what `forward_request` reports back
  to the main agent.
- **No way around a closed gate.** `RouteGates::from_spec(spec)` holds each
  route's gate; `gate_status(route, input)` is `Open` or `Closed { missing }`,
  an unknown route closed with `unknown_route`. A route is dispatched only
  through an `OpenRoute`, which only `RouteGates::open` constructs and only
  for an open gate.

**Type checks** (on save and publish, `Cond::type_check`). Each leaf must be
able to hold: every `eq` and `in` value must pass the slot's own validator,
and `provenance` must be in the slot's `set_by`.

**The trusted-gate rule.** A gate that only checks model-written slots cannot
guard a subject-bound call. So a route needs a gate that cannot open without a
verifier- or host-written slot — a non-`llm` `provenance` leaf on every way
through it (any child of an `all`, every child of an `any`, never under a
`not`; `Cond::requires_trusted_provenance`) — when it binds from state, or
when its target, or any agent below it, binds a tool. A route that binds from
slot `s` must gate on a non-`llm` provenance *of `s` itself*
(`Cond::requires_trusted_provenance_of`).

A model check on what the visitor asks is the topic guard
([`agent-runs.md`](agent-runs.md#topic-guard)); it can only refuse, never open
a gate.

## Schemas without a JSON-Schema crate

`finish` schemas and `subject` slot schemas are checked by the finish
contract's validator, which supports a fixed subset: `type` (one name or a
list), `properties`, `required`, `enum`, `items` and a boolean
`additionalProperties`; the annotations `title`, `description`, `default`,
`examples` and `$schema` are ignored. Slot constraints (`min_length`,
`pattern`, `minimum`, …) are the slot types above, checked by `SlotDef`.
`pattern` uses the existing `regex` dependency.

An unsupported keyword is a **validation error on save**, never silently
ignored, so a schema can never under-validate. That is the reason not to
accept arbitrary JSON Schema ([`tools-rbac.md`](tools-rbac.md#finish-contract)).

Tests: `agents/spec.rs` and `spec/model.rs` (the walk, the types, their
agreement), `agents/state.rs`, `agents/gate.rs`, `agents/slot_tools.rs`,
`tests/it/agents.rs` (drafts, publish, shares over HTTP).
