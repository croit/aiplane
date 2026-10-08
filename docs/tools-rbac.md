# Tools + RBAC

How server-side tools work. For the list of *which* tools exist and when each
is registered, see [`tools-inventory.md`](tools-inventory.md).

This document names **symbols, not file paths or line numbers** — the crate
layout is actively being reshaped (see the crate-split work), and a doc pinned
to paths goes stale on the next move. `grep` for a symbol; it will be wherever
it lives today.

## What a tool is

A **tool** is a Rust handler AIplane runs on behalf of an LLM during a chat
completion. From the model's side it's an ordinary OpenAI function-calling
tool: it has a JSON schema, the model emits `tool_calls`, AIplane executes
them, and the result feeds the next round. The model doesn't know the tool ran
on AIplane.

This is **not** a passthrough and not an MCP broker. AIplane *is* the tool
runtime. (It *also* bridges MCP servers — see "Tool sources" — but that is one
source of tools among several, not the architecture.)

### Why server-side

- Tools reach internal systems (databases, APIs, file stores) we don't want to
  expose to every model client.
- Running them here means we control the inputs, can rate-limit, and can audit.
- Clients need no extra wiring. Any OpenAI SDK sees a normal completion.

## The `Tool` trait

```rust
pub trait Tool: Send + Sync + 'static {
    fn id(&self) -> &str;
    fn schema(&self) -> ToolDef;
    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a>;
    fn max_duration(&self) -> Option<std::time::Duration> { None }
}
```

- `ToolFuture<'a>` is `Pin<Box<dyn Future<Output = ToolResult> + Send + 'a>>`,
  hand-rolled rather than pulling `async_trait` in for one trait. (`async_trait`
  *is* a dependency elsewhere — `SessionDriver` needs it — but the tool trait
  doesn't use it.)
- `id()` returns `&str`, **not** `&'static str`: MCP- and ComfyUI-bridged tools
  build their ids at runtime from server / workflow names.
- `schema()` is generated per call so it can interpolate runtime context. Most
  tools return a constant.
- `max_duration()` overrides the runner's per-tool timeout. The sandbox tools
  use it — their jobs (LibreOffice, headless Chromium, cold start) legitimately
  exceed 30s.

### Tool ids are a contract

`ToolRegistry::with` asserts at registration that an id matches OpenAI's
function-name regex `^[a-zA-Z0-9_-]{1,64}$`, and panics on a duplicate.

That assertion exists because a `.` in an id (a dotted namespace like
`company.echo`) *silently* breaks against strict tool-call parsers — qwen3-coder
is the one that bit us: the parser either drops the call or rewrites the name
before emitting `tool_calls`, and the symptom looks like the model ignoring the
tool. Use `_` for namespacing. Failing at boot beats shipping a tool that only
breaks on some upstreams.

MCP tool ids are derived from the connector and remote tool names. A short id
that is already valid stays readable as `mcp__<server>__<tool>`; otherwise the
adapter appends a digest of the original pair after sanitizing or shortening it.
That keeps arbitrary remote names within the wire contract without silently
merging two distinct MCP tools.

### `ToolContext`

Carries the caller's identity plus the handles a tool may need, so adding a
dependency doesn't change the trait signature:

- **Identity / RBAC** — `principal` (a person, `Principal::User { id, roles }`,
  or a system principal with its grants — see
  [System principals](#system-principals)), `token_id`, `client_ip`. Scope rows
  by `principal.subject_id()`. A tool that acts *for a person* (memory,
  `notify_user`, `schedule_action`, `get_user_location`, `browser_control`) goes
  through `ctx.person(tool_id)`, which refuses with a message naming the
  principal when there is no person behind the call.
- **Agent run** — `agent: Option<Arc<AgentRun>>`, the one value an agent run
  carries (its principal, call chain, finish contract, budget and injection
  scan) when the call is part of one (`ctx.agent_active()`, `ctx.chain()`);
  `None` everywhere else. See [`agent-runs.md`](agent-runs.md#the-call-chain).
- **Storage** — `db` (the SQLite pool), `s3` (chat attachments; `None` without
  `[chat.s3]`), `crypto` (the at-rest key, for tools that read a sealed
  operator setting).
- **Conversation** — `assistant_turn_id`, `session_id`: `Some` only on the
  chat-UI path. The `/v1` proxy paths have no session, which is what
  `requires_chat_session` exists for (below). `attachment_reservations`
  serialises filename picks across tool calls that run concurrently in one
  round.
- **Optional subsystems** — `geoip`, `indexer` (RAG), `image_gen`,
  `sandbox_lease` (one container per turn, so successive `run_in_sandbox` calls
  share `/work`), `chat_feedback` (push UI onto the live SSE stream and await a
  browser reply — `get_user_location` asks for a position, `ask_user` asks a
  question; one hub per reply shape, so the two endpoints can't un-park each
  other's tool), `push` (Web Push, for `notify_user`).
- **Pausing** — `suspend`: `Available` on the interactive chat path and on
  agent runs, where a tool may pause the turn for a decision; `Decided(…)`
  when the call runs again after one; `Unavailable` everywhere else (`/v1`,
  scheduled and webhook runs). See
  [Suspend and resume](#suspend-and-resume).
- **The grant** — `granted_tools`, the tool ids the principal may run, set by
  the chat driver once per turn. `enable_tools` refuses keys outside it; `None`
  (the `/v1` paths, tests) means nothing granted.
- **The current model** — `model`, when the path resolved one. Carried so a
  tool that creates work to be *run later* can inherit it rather than guessing a
  pool id: `schedule_action` gives the action it writes the same model the user
  is talking to.

Two of these carry a **per-turn budget**, not just a handle, because the
context's lifetime is exactly one turn and that is the window a limit needs:
`attachment_reservations` (a mutex, so concurrent uploaders can't pick the same
filename) and `push` (a latch — `PushNotifier::claim` succeeds once, so a model
in a tool loop can't turn someone's phone into a notification feed). Neither
belongs in `server::limits`, which counts tokens and cost per user over time and
has no notion of a turn.

Every optional field is `None` where that subsystem isn't configured, and tools
are expected to degrade with a clear message rather than assume. Tools never
receive the caller's OIDC access token — acting *as* the user against a
downstream service is an explicit per-integration concern (see the per-user MCP
connectors).

There is no `tracing::Span` field: per-tool spans are created by the runner
around `run`, not threaded through the context.

## Registration

Tools are registered in `main`, into a `ToolRegistry`:

```rust
let registry = ToolRegistry::new()
    .with(tools::echo::Echo)
    .with(tools::search_web::SearchWeb)
    .with(tools::fetch_attachment::FetchAttachment::new(sandbox.clone()));
```

We do **not** auto-discover tools at runtime. Adding one means writing code,
opening a PR, and reviewing it — which is the point.

Many tools are registered conditionally (no GeoIP database → no `lookup_ip`),
so the model is never offered a tool whose every call could only answer "not
configured". The gate for each is in
[`tools-inventory.md`](tools-inventory.md).

### Presentation metadata

Two lookup tables in `catalog` decide how a tool appears on `/tools`:
`category_for` (which group) and `display_meta` (hand-written plain-language
title and description — the schema description is written for an LLM and reads
as jargon in a settings list). Both fall through gracefully: an unknown id
lands in `Category::Utility` with its schema text. That makes forgetting them
*silent*, so the inventory drift guard asserts every registered id has a real
category and hand-written copy.

Internal plumbing is kept off the toggle surfaces by `is_hidden` —
`company_echo` (the loop's smoke test) and `enable_tools` (whose toggle would
be inert, since the session allow-list force-keeps it).

## Tool sources

The runner resolves tools through `ToolSource`, not the concrete registry:

```rust
pub trait ToolSource: Send + Sync {
    fn get(&self, id: &str) -> Option<Arc<dyn Tool>>;
    fn defs_for(&self, allowed: &[String]) -> Vec<ToolDef>;
    fn ids(&self) -> Vec<String>;
    fn contains(&self, id: &str) -> bool { self.get(id).is_some() }
}
```

`ToolRegistry` implements it, and so does `CompositeToolSource`, which overlays
per-request tools on top: a user's connected MCP connectors
(`mcp__<server>__<tool>`) and the hot-reloadable ComfyUI workflow catalog
(`comfyui_<workflow>`). One seam means the buffered `/v1` loop, the streaming
`/v1` loop, and the chat-UI driver all gain per-user tools identically.

`SlotTools` (`aiplane-runtime::agents::slot_tools`) is a third source: the
generated `set_<slot>` tools of one agent's state, one per slot the model may
write. They exist only for an agent run and need no grant
([`agent-spec.md`](agent-spec.md#state)).

`RunToolSource` (`aiplane-runtime::agents::profile`) is how an agent run sees
all of them. It wraps the turn's grant-narrowed source and adds the run's
synthetic tools (`set_<slot>`, `forward_request`) on top. It also wraps every
granted tool that has **bound arguments** in a `BoundTool`
(`agents::bind`): the bound parameters are removed from the schema the model
sees, and on every call the gateway's values overwrite whatever the model sent.
Binds are mapped explicitly per tool (`tool_resources.<tool>.bind: {param:
source}`). A `const` bind is fixed in the spec. A `state.<slot>` bind is read
from the conversation state when the call runs, and the call is refused while
the slot is unset. A `route.<name>` bind takes the value the dispatching route
passed. A tool that declares a *subject parameter* (one another tool binds from
state or a route) without binding it is withheld: not offered, and refused if
called. Without an agent run the source passes `inner` through unchanged. The run offers only the spec's `main.tools` that the
principal is also granted, plus the synthetic ones
([`agent-runs.md`](agent-runs.md#the-router)).

## Lazy tool disclosure (`enable_tools`)

The defining behaviour of the current design, and the thing most likely to
surprise: **tools start off**. A conversation's advertised set is

```
allowed_tools_for_session = (RBAC-granted ∩ registered)
                            ∩ ({enable_tools} ∪ per-conversation enabled)
```

Short tool lists are cheaper and models pick from them more accurately. When a
request needs a capability the model doesn't currently have, it calls
`enable_tools` with one or more **toggle keys**; that writes per-conversation
rows, and the real schemas appear from the next round on and stay for the rest
of the conversation. Calling a **granted** tool directly without enabling it
first still works — it just costs a round: the driver runs the call and writes
the enablement row itself (`source = "auto-call"`).

Neither path can reach past the grant. `enable_tools` skips (and reports in
`skipped`) any key that covers no tool the principal is granted — it reads the
grant from `ToolContext::granted_tools`, which the driver resolves once per
turn, and treats an unresolved grant as empty. The driver resolves and runs
every call through a `GrantedToolSource` holding exactly
`AppState::granted_tool_ids` (RBAC grant, ComfyUI expansion, minus the user's
`/tools` switches, plus the granted tools of the MCP overlay), so a name outside
it is answered with the ordinary "No tool named …" error the model can read —
no run, no enablement row. A `chat_session_tools` row only ever narrows the
grant, so a row left behind after a grant is revoked neither offers nor runs
anything.

Two tools bypass the gate:

- `enable_tools` itself is force-kept (`BOOTSTRAP_TOOL_ID`), or the model could
  never turn anything on.
- `read_skill` is force-kept *when the caller has at least one permitted
  skill*, because the system prompt advertises those skills every turn; making
  the model enable the loader first would be pointless friction. With no
  permitted skills it stays lazy.

Related tools collapse onto **one** toggle key, because users reason in
capabilities rather than function names: `remember` + `recall` → `memory`; the
canvas tools (including `export_document`) → `document`; a typst template's
render / edit / read / pptx family → its render id; all `comfyui_*` →
`comfyui`; all of one MCP server's tools → `mcp__<server>`; `offer_download` +
`zip_attachments` → `upload_attachment`. `entry_key_for` maps id → key,
`retain_enabled` applies a disabled set.

Every surface that lists toggles (`/tools`, the chat composer's picker, a
token's panel) renders `catalog::entries`, and every surface that enforces one
(discovery, the per-conversation overlay, token states, RBAC) goes through
`entry_key_for`. They have to agree key for key: a listed key nothing looks up
is a switch that does nothing — `offer_download` and `zip_attachments` were
exactly that, each listed on its own while `upload_attachment` governed them.
`every_listed_toggle_is_the_key_discovery_and_enforcement_use` in
`crates/aiplane/src/tool_registry.rs` holds the two sides together over the
real registry.

The advertised order is deliberate: `enable_tools` first (identical across
every conversation), then the tail sorted by toggle key then id. That keeps the
serialised tool block byte-stable, so list churn doesn't invalidate the
upstream prompt cache.

## RBAC

Roles come from the OIDC roles claim (configurable — see [`auth.md`](auth.md)),
mapped onto internal role ids. A user's effective tool set is the **union over
their roles**, and grants come from several sources that are unioned:

- **Static config** — each role's `tools` list in `[[roles]]`; `["*"]` means
  everything *registered*. `*` never grants a tool that doesn't exist, and a
  granted id that isn't registered at boot is logged and ignored (fail-soft, so
  a stale config can't block startup).
- **Groups** — `/admin/groups` maps OIDC claim values onto gateway groups with
  their own tool and skill grants. Pools, RAG collections, and MCP connectors
  restrict access by group, and those saves reject a group name that matches no
  group: stored verbatim it would hide the resource from everyone rather than
  reserving it for someone.

Grants are matched **exactly**. There is no glob syntax, so `some*` is looked up
as that literal id and grants nothing. Note that `is_admin` does **not** imply
tool access: `allowed_tools` never consults it, so an admin group still needs its
grants (the setup wizard seeds `admin` with `tools = ["*"]` for exactly this
reason).

### Family grants

Besides ids, three values stand for a whole **family**. They are the toggle keys
from `tool_naming`, and they are *late-binding*: unlike a list of ids, they keep
covering items added later. That matters because two of these families change
without a deploy.

| Value | Covers | Why an id list is not enough |
|---|---|---|
| `*` | every registered tool, plus every ComfyUI workflow | — |
| `comfyui` | every ComfyUI workflow, present and future | workflows are created by operators at runtime and are in no registry |
| `mcp__<server>` | every tool of that MCP connector | the tool list is the server's to change |

`typst_<id>` is deliberately **not** a family grant. Typst templates are
discovered at boot and registered as ordinary tools, so per-template and
per-variant (`_edit` / `_read` / `_pptx`) grants already work through the normal
path — and the family key is identical to the render tool's own id, so treating
it as a family would silently widen an existing grant.

Neither `comfyui` nor `mcp__…` is a registry id, so both fall through
`allowed_tools` untouched; they are resolved by `grants_comfyui_overlay` and
`mcp_grant` respectively.

### MCP: connector ACL, then tool grants

MCP is gated twice, and the two do different jobs:

1. **Which connectors** the caller reaches — the connector's `allowed_groups`,
   checked in `layer_for_user`.
2. **How much of a reached connector** they see — `Resolver::mcp_grant`.

The second only ever narrows the connector ACL. A caller whose groups contain
no `mcp__…` grant is denied MCP tools. A `*` grant or an admin group permits
all tools of a connector the caller can reach. Otherwise a tool survives only
if its own id or its `mcp__<server>` key is granted. A token owner can then
choose Off, Auto, or On for that connector in the token picker.

Per-user MCP connections and their cached tool definitions are keyed by
`(user_id, connector_key)`. Two users can see the same tool id while receiving
different definitions and executing with their own OAuth credentials. The
shared `mcp_connector_tools` cache below is only for the admin grant editor;
request-time schemas and execution come from the caller's live user layer.
Global connectors intentionally use one shared credential, but still apply
each user's RBAC, connector ACL, tool modes, and token states separately.

Authoring per-tool grants needs a tool list, and an MCP server's tools are only
visible while connected on some user's behalf — which an admin editing a group
cannot do. So `mcp_connector_tools` caches what each connector reported the last
time anyone connected, and the group editor offers that. It is a cache, not a
source of truth: a connector nobody has connected has nothing to offer yet (the
editor says so), and a stale row is harmless because grants are matched against
the live tool id at call time.
- **Per-user toggles** — each user turns their granted tools on and off on
  `/tools`.
- **Per-token scoping** — a `gwk_…` token has the same Off / Auto / On
  capability picker as chat, minus the capabilities whose every tool is
  chat-only (`catalog::api_keys`): the `/v1` path drops those before a token's
  choice is read, so their switch would do nothing. The token-tools endpoint
  refuses the same keys. It includes connected MCP integrations and
  permitted skills. Missing built-in tools default to Auto; missing MCP
  integrations and skills default to Off. Explicit Auto is stored for the
  latter two families. The token's master tool switch remains an outer gate.
- **Skills** — a role's `skills` list plus a per-skill grant editor in the UI;
  `read_skill` rides along for any role granted a skill.

```toml
[[roles]]
id = "engineering"
models = ["*"]
tools = ["search_web", "rag_search", "run_in_sandbox"]

[[roles]]
id = "admin"
models = ["*"]
tools = ["*"]               # everything registered
```

The layers compose in one direction only: RBAC (roles + groups) decides what a
user *may* use; the per-conversation, per-token, and per-user layers can only
subtract.

**Invariant: a tool runs only if the acting principal is granted it, however
the model names it.** Offering is not the gate — a model can call any name it
read in training or was prompted with. Every loop therefore resolves calls
through a source already narrowed to the grant: the chat driver (and its resume
path) through `GrantedToolSource`, the `/v1` loops through
`DiscoverableToolSource` built from the token's RBAC-bounded layer. An ungranted
name is answered as unavailable in chat and handed back to the client on `/v1`
(where it is indistinguishable from a client-owned tool). Pinned by
`a_chat_model_naming_an_ungranted_tool_is_refused_and_does_not_enable_it` and
`a_v1_model_naming_an_ungranted_gateway_tool_does_not_run_it` (plus its
streaming sibling).

The chat driver's per-call decision (`openai_driver/call_policy.rs`) is taken
against that same narrowed source: a name the full registry knows but the grant
does not is `not_granted` for a system principal (audited as such in an agent
run) and `unknown_tool` for a person — one check, so the audit row and what
actually ran can never disagree.

Run-scoped synthetic tools are not grants and sit outside the filter: an agent
run's `set_<slot>`, `forward_request` and `finish` tools come from
`RunToolSource`, which the driver layers *over* the `GrantedToolSource`, never
inside it. Their existence for the run is
the permission; nothing a person or principal is granted can reach them, and
they reach nothing outside the run. A bound tool is still a granted tool: the
binding wraps what the grant filter returned and can only narrow its arguments.
A sub-agent that `forward_request` starts gets its own `GrantedToolSource`,
built from its own principal's grants. Nothing the calling agent holds passes
down.

### System principals

A `gws_` token resolves to a **system principal** ([`agents.md`](agents.md#principals)), and
none of the above applies to it. It holds exactly the rows in
`principal_grants`, one resource each:

| Grant kind | What it unlocks | What it does *not* get |
|---|---|---|
| `tool` | that registry tool (or loaded `comfyui_<id>` workflow) | default groups, `*`, the `enable_tools` bootstrap, anything `requires_chat_session` |
| `connector` | every tool of that **global** or **agent** connector | per-user connectors — `user_mcp` is never read, so no person's OAuth connection is reachable; connector `allowed_groups` does not apply (for an `agent` connector it decides who may grant it) |
| `skill` | that global skill | anyone's private skills |
| `rag_collection` | that collection, by id | collections with empty `allowed_groups` ("open to everyone" means everyone *person*) |
| `model` | that model, by the name the chat picker shows: a model id, a backend alias (and what it resolves to), or an automatic-route alias (which also reaches the route's candidates, fallback and selector) — only through the pools the grant records: every pool of the model's kind the granting manager could use, every serving pool for an admin's grant; a regrant only widens them, and a non-admin's token narrows them to its minter's pools. The caller must name a granted name itself: a granted alias authorises its target only when the gateway resolves the alias ([`agents.md`](agents.md#models)). Any kind a request routes by name can be granted — chat, transcription, speech, image, embedding, rerank, System One; OCR models cannot | any other model, open pools included; a pool the granting manager could not use; `is_admin` bypass. No grant, no model |
| `a2a_caller` | calling that agent (ref: its id) over A2A, `/a2a/agents/{id}` ([`agent-a2a.md`](agent-a2a.md#serving-an-agent-over-a2a)) | anything of the agent's own: the task runs as the agent's principal, with the agent's grants, never the caller's |
| `a2a_agent` | handing a route's task to that external A2A agent (ref: its agent card URL); only an admin can grant it ([`agent-a2a.md`](agent-a2a.md#external-agents-as-route-targets)) | any other card URL, or an endpoint or token URL outside the card URL's origin |

The principal-aware entry points on `AppState` are
`allowed_tools_for_principal`, `allowed_skills_for_principal`,
`pool_access_for_principal`, `mcp_layer_for` and `mcp_grant_for_principal`;
`Resolver::principal_resource_allowed` / `principal_skills` do the same for
tools that check a resource themselves (`rag_*`, `read_skill`). On the `/v1`
path every granted tool is offered directly — there are no token tool prefs and
no Auto disclosure for a principal; the grants are the whole policy.

The same holds for a headless run as a principal (`headless::drive` with
`Principal::System`, the agent path): its offer is its grants, and a call to a
registered tool outside them is answered with a `not granted` refusal instead
of the chat path's auto-enable. Inside an agent run every call's decision is
also written to `agent_audit` with the call chain
([`agent-runs.md`](agent-runs.md#the-call-chain)).

**Who may grant.** Users whose groups have `can_manage_agents` (admin implies
it), through `/api/v0/system-principals/*`. A grant is refused unless the
manager holds the resource *at that moment*, checked with the rule that decides
their own access (`allowed_tools` + ComfyUI expansion, connector
`allowed_groups` + MCP grant, `allowed_skills`, `resource_allowed`, and for a
model the manager's own chat/transcription/speech model list —
`server::model_choices::offered` — where an automatic route counts only when
the manager may use every candidate and the selector too). After that the grant belongs to the principal: it is never re-derived
from the manager, so it survives the manager losing rights or leaving. Every
change is written to `agent_audit` in the same transaction.

## Tool injection

On `POST /v1/chat/completions`, `/v1/responses` and `/v1/messages`:

1. Compute the caller's allowed set (roles → ids → resolvable in the
   `ToolSource`).
2. Drop anything `requires_chat_session` — the proxy paths have no chat turn,
   so advertising those would hand the model a guaranteed error rather than a
   completion. It is a single source of truth precisely so the advertise filter
   can't drift from the runtime gate; it has drifted before.
3. Apply the token's capability states to built-in tools, MCP connector families,
   and skills. **Off** is excluded from schema lookup and execution. **On** is
   advertised immediately. **Auto** is reachable through the small
   `search_gateway_tools` definition; a matching search exposes at most five
   schemas in the next model round. The search and all tools it activates are
   confined to the token's RBAC and per-user grants. A token with its master
   switch off takes the byte-dumb path.
4. If the request body already carries `tools`, **union** with the offered set,
   de-duped by `function.name`. Client-supplied tools are never executed here —
   they round-trip to the client like normal OpenAI tools, so gateway tools and
   client tools coexist in one completion.
5. Leave `tool_choice` alone when it is `"required"` or names a tool.
6. Forward upstream. Every returned gateway tool call is resolved through the
   same offered source, so a known but disallowed tool cannot run.

`requires_chat_session` covers two different reasons a tool needs the chat path,
and it is worth keeping them apart when deciding whether a new tool belongs
there:

- **Nowhere to put the output.** `upload_attachment`, `generate_image`,
  `export_document` — they attach something to a turn that doesn't exist on
  `/v1`.
- **Nobody to ask.** `ask_user` needs a human watching the stream, and
  `schedule_action` and `delete_scheduled_action` need a person's approval
  before writing. That approval exists because a scheduled action later runs
  **as the user**, unattended, until removed — persistence is what makes
  prompt injection there worth a human "yes", where an ordinary tool call
  isn't. It is the durable pause of the next section, so a scheduled run that
  calls them waits for its owner in the inbox rather than writing anything.

The inverse case is worth stating too, since it is the easy mistake: a tool
whose *optional* argument needs a session does **not** belong here.
`render_typst` renders inline `source` anywhere and only needs a session for its
`document_id` path; marking it chat-only would remove a working capability from
`/v1` to protect an argument a proxy caller has no use for. Reusable pattern:
a tool that needs a person's yes wraps itself in `AskFirst`, or, when it must
check its arguments before asking, calls `ask_first::approval` itself.

## The tool-call loop

When a response carries `choices[*].message.tool_calls`:

```text
   ┌───────────────────────────────────────────┐
   │  classify the turn's tool_calls:          │
   │     gateway-owned = resolvable in source  │
   │     client-owned  = any other name        │
   ├───────────────────────────────────────────┤
   │  if NO gateway-owned:                     │
   │     return response to client (it drives) │
   │  elif ANY client-owned (mixed turn):      │
   │     return whole turn to client unchanged │
   │  else (gateway-owned only):               │
   │     run tools, append {role:"tool", …},   │
   │     re-POST upstream with extended msgs,  │
   │     repeat                                │
   └───────────────────────────────────────────┘
```

**Why a mixed turn yields to the client.** On the proxy path the *client* owns
the conversation history — it re-sends every message each request. We can only
run a turn fully server-side when that turn calls our tools and ours alone. If
one assistant turn calls both a gateway tool and a client tool, we can't run
ours *and* hand control back mid-turn without either dropping the client's call
or leaving it unanswered in the next upstream round (which the upstream
rejects). So we hand the entire turn back; the client runs its tool and
re-submits, and the model re-emits the gateway call on a later gateway-only
turn. Mixed turns are rare in practice — models seldom batch a gateway and a
client tool in one turn.

### Bounds

- **Rounds per turn** — `MAX_TOOL_ROUNDS` = **16** for both `/v1` loops
  (buffered and streaming, as a `runner::RoundBudget`). This is a compile-time
  constant and not configurable. The chat-UI driver takes its cap from the
  conversation's effort level instead (`Effort::max_rounds`; the default, low = 16).
  All three close the budget the same way, through
  `runner::prepare_final_round`. The last round tells the model in the system
  message to answer from what it has. It then either sends `tool_choice:
  "none"` or withholds the tools, according to
  `ServingProfile::honors_tool_choice`; Ollama, vLLM and SGLang get the tools
  withheld. A call the model makes anyway, structured or written out as
  `<tool_call>` text, never runs, and the text is cut from the reply. If
  nothing else was written, the model gets one closing round with no tools
  (`prepare_closing_round`). On `/v1` the answer comes back as a normal
  completion carrying the `aiplane.tool_budget_exhausted` signal, and an empty
  closing round fails as `502 tool_budget_exhausted`. In the chat UI an empty
  closing round ends the turn with the "ran its tools but wrote no answer"
  notice. See
  [`gateway-api.md`](gateway-api.md#tool-round-budget).
- **Repeated identical calls** — `repeated_calls::RepeatedCallGuard` counts
  gateway-owned calls per turn by (tool id, canonical arguments: object keys
  sorted, whitespace ignored, empty or unparseable arguments read as `{}`).
  Any different call in between (calls within one round count in array order) resets the count, so edit, read, edit, read never trips. The first `MAX_IDENTICAL_CALLS` (3) in a row run. The next `MAX_REFUSED_CALLS` (2) do
  not run; the model gets a tool error saying it already has that result and
  should use it. The call after that stops the turn. The guard is wired through
  `runner::execute_tool_calls_guarded` in all three loops (buffered `/v1`,
  streaming `/v1`, chat driver). It catches what `LoopGuard` cannot: a model
  that writes no repeated text, only the same well-formed call every round.
  Buffered `/v1` ends like an exhausted round budget (one closing round without
  tools, `aiplane.tool_budget_exhausted`) and adds `aiplane.stop_reason` naming
  the tool. The streaming `/v1` loop ends the stream with an error chunk. The
  chat UI aborts the turn with the same reason as its error text. Every stop
  logs a `warn` with the tool and round. The reason is English-only, like
  `LOOP_MESSAGE`, because it is written into the turn row at generation time.
- **Per-tool timeout** — 30s, overridable per tool via `max_duration`.
- **Concurrency** — tool calls within one round run concurrently, bounded by a
  per-request semaphore of 4. The exception is an agent run's synthetic tools:
  `ToolSource::phase` tags each call with a `ToolPhase`, and the runner runs the
  state writers (`set_<slot>`, verifiers) first and one at a time, then the
  concurrent rest, then the tools that act on state (`forward_request`,
  `request_human`) one at a time, then the `Terminal` one (`finish`), which
  the driver only lets run as the round's sole call. Only `RunToolSource`
  returns anything but `Concurrent`. Results keep call order whatever order the calls ran in
  ([`agent-runs.md`](agent-runs.md#synthetic-tools)).
- **Tool-result context budget** — once cumulative `role:"tool"` content passes
  128 KB (`/v1` loop) or the turn's allowance derived from the model's context
  window (chat driver), older large results are replaced by re-callable stubs
  while the last three stay verbatim. Both loops call the one
  `stub_old_tool_results`. A stub keeps the first 300 characters of the result
  and its `full_output_ref`, so the model can tell what it was and re-run it.
  Every `tool_call_id` stays answered, only the content shrinks, and a stub is
  below the stubbing threshold so a second pass changes nothing. It triggers on
  size only, so short conversations keep the full history and the prompt cache
  intact (clearing would invalidate the cached prefix). Only the replayed
  messages shrink; the stored turn keeps the full results.

### Run budgets

Every chat-driver run carries an `aiplane_runtime::budget::Budget { rounds,
seconds, tokens }`. A chat turn derives it from the conversation's effort level
(`Budget::from_effort`: the `Effort::max_rounds` cap, no time or token limit). An agent run may carry one
(`AgentRun::with_budget`); `Budget::new` clamps its rounds to `1..=HARD_ROUND_CAP`.
`seconds` and `tokens` are optional (`None` = unlimited). An agent run takes
its budget from `main.budget` in its spec (rounds default to the cap of the
default effort, `low`). A sub-agent run gets its own budget from its own spec, never a
share of its parent's: the parent's `forward_request` call waits for the
sub-agent while the sub-agent's rounds count against the sub-agent alone.

- **Tokens** are the upstream-reported usage (the `total_tokens` of each
  round's trailing usage frame, else prompt + completion), summed over the
  run's rounds. A run with a token limit always requests `include_usage`, even
  when metrics and compaction are off. An upstream that reports no usage
  counts as zero.
- **Seconds** are wall clock from the start of the run, read through the
  driver's `Clock` (a test seam; production is `Instant::now`).
- Time and tokens are checked **between rounds**, before each request is
  built, never mid-stream: aborting a reply would discard output already paid
  for and leave a half-written message. A run can overshoot by at most one
  round.
- Running out of *any* limit makes the next request the final round, exactly as
  running out of rounds does (tools withheld, or only `finish` offered under a
  contract). A contracted run that does not finish there settles as
  `Incomplete` with `round_budget_exhausted { rounds }`,
  `seconds_exhausted { seconds }` or `tokens_exhausted { tokens }`, naming the
  limit that hit. A run always gets at least one request.

### Injection scanning of tool results

What an MCP server, RAG, a web fetch or a database returns may have been
written by an outsider, so a gateway-owned result is screened before it becomes
a `role: tool` message. The hook is `runner::screen_result`, called at the end
of `execute_tool_calls`, so the chat driver, the headless runs, the resume path
and both `/v1` loops share it. Client-owned calls never pass through the
gateway and are not scanned. `server/tools/injection.rs` holds the rest.

An agent run carries an `InjectionScan { policy }`
(`AgentRun::with_injection`); every other turn runs with the default.
`RunProfile` sets `Flag` for every agent run,
main agent and sub-agent alike, so a sub-agent's `finish` result reaches the
main agent screened like any other tool result. The
default is `Off`, which skips scanning entirely, so the result reaches the model
byte for byte. The `/v1` loops pass `Off`.

| Policy | What the model sees for a result with a hit |
|---|---|
| `Off` | the result, untouched (and nothing is audited) |
| `Flag` | `{"untrusted_tool_output": {notice, tool, signals, data}}`: the original under `data`, with a notice that it is data and not instructions. A `tool_content_parts` result keeps its shape; the notice is a leading text part |
| `Redact` | the result with each matched span replaced by `[removed: possible prompt injection]` |
| `Drop` | `{"error": "The result of `<tool>` was withheld ..."}` |

**Heuristics** (`scan_text`, no dependency beyond `regex` and `base64`).
Case-insensitive, English and German, run over every string of the JSON result
after decoding, so `\u200b` escapes are seen. The families (`Signal`):
`ignore_instructions` ("ignore all previous instructions"), `role_override`
("you are now a ...", "from now on you must", "du bist jetzt ein"),
`system_prompt_probe` ("reveal your system prompt", "new instructions:"),
`role_markup` (`<|im_start|>`, `[INST]`, `<<SYS>>`, `<system>`), `role_prefix`
(a line starting `system:` or `assistant:`), `hidden_text` (zero-width space,
word joiner, BOM, bidi overrides, Unicode tag characters; the joiner/non-joiner
used by emoji and Persian are deliberately allowed), `encoded_payload` (a base64
run of 60+ characters that decodes to text matching another family),
`tool_request` ("you must now call the X tool"), `secret_request` ("send me the
API key"), `exfil_url` (an instruction verb next to a URL with a query
parameter, or a URL whose parameter is a placeholder such as `{{data}}`). Image
parts are never scanned. The set is a tripwire for lazy attacks and will
produce false positives in text that *discusses* injection; each pattern has a
test, and so do clean code, JSON, prose and a German letter. Add a pattern with
a failing fixture first, and a clean fixture if it is broad.

**No model-based layer.** The scan is the regex set alone; no model judges a
tool result. A model check on what a visitor may ask is the agent's topic guard
(`agents::topic_guard`, a side call).

**Recording.** Every hit logs a `warn` (tool, principal, policy, signals). When
the acting principal is a system principal it also writes an `agent_audit` row of
kind `injection_detected` (no acting user; `detail` has `tool`, `call_id`,
`policy`, `signals`, never the matched text). A person's run is logged only.
Suspension requests (`extract_suspend`) are never rewritten.

### Finish contract

An interactive turn ends when a round comes back without tool calls. That is
not a result for a run nobody watches, so a non-interactive run can be given a
`aiplane_runtime::finish::FinishContract` — a JSON schema — and then ends in
exactly one of two ways: a schema-valid `finish(result)` call
(`RunOutcome::Finished { result }`), or a structured
`RunOutcome::Incomplete { reason, summary }`. The contract belongs to an
agent run (`AgentRun::with_contract`; `RunProfile` gives one to every routed
sub-agent), and `headless::drive` returns the outcome. Runs without a
contract — every chat turn, every scheduled action and webhook, every `/v1`
request — end when a round comes back without tool calls, and no `finish`
tool is offered to them.

`finish` is a real tool: `finish::FinishTool`, owned by the run's `AgentRun`
and offered through `RunToolSource` in the `ToolPhase::Terminal` phase. It is
dispatched, audited, guarded against repeats and recorded on the turn's tool
rows like any other call. The driver has no name-based case for it; it knows
the terminal phase:

- A terminal call made next to other calls is refused ("call it on its own").
- A terminal call that succeeds ends the turn.
- A contracted run's final round offers only terminal tools.

Inside the chat driver, with a contract:

- Every round offers `finish` alongside the run's tools, and the leading
  system message says the run ends only through it.
- A round of text without `finish` does not end the run. The text is replayed
  with a user-role nudge, and the round counts against the budget.
- A `finish` call on its own is validated (`FinishContract::check_args`). A
  valid one ends the run, and the tool keeps its result. An invalid one fails
  like any tool, so its tool slot is answered with every validation error
  (location and cause), and the run continues. Arguments that are not a JSON
  object reach the tool as `{}` (the runner normalises them) and read as a
  missing `result`. A `finish` made in the same
  round as other calls is refused ("call it on its own"), because ending
  there would throw away the other calls' results unread.
- The final round offers *only* `finish` (the round's tool list is narrowed
  to the terminal phase and `tool_choice` dropped: `"none"` would forbid the
  one call that matters). Any other call the model makes there never runs. The model is told to call it or write what is left undone.
  Anything but a valid `finish` there ends the run as
  `Incomplete { reason: round_budget_exhausted { rounds } }` (or the
  `seconds_exhausted` / `tokens_exhausted` of a [run budget](#run-budgets)), with the model's
  last text as `summary`, or a gateway-written account of the rounds and tools
  when it wrote none. No closing round follows. The turn carries a notice.
- A contracted run still passes through the repeated-call guard. A guard stop
  settles as `repeated_tool_call { tool }`, with the stop message as `summary`.
- An output-token cutoff settles as `output_truncated`.

**One conversion.** No exit path settles the run itself. The round loop
(`run_one_turn`) returns a `TurnEnd`: `Ran(TurnOutcome)` when it ran its
course (an answer, an accepted terminal call, a pause, a cancel), or
`CutShort { reason, summary, turn }` when a limit stopped the run early (the
final round, an output cutoff, a repeated-call stop), where `turn` is what the
worker records. `OpenAiDriver::run_turn` turns that, or the `TurnError`, into
the `RunOutcome` in one place (`run_outcome`) and settles it on the
`AgentRun`:

| How the turn ended | `RunOutcome` |
|---|---|
| a `finish` call was accepted, whatever followed | `finished { result }` |
| `CutShort` | `incomplete` with its `reason` and `summary` |
| any `TurnError` | `failed { message }`, the error as the turn row shows it |
| `Ran`, cancelled | `cancelled` |
| `Ran` otherwise (e.g. a pause) | `failed { message: "the run ended without a finish call" }` |

So a new exit path settles by construction: returning an error or a plain
`TurnOutcome` already maps to an outcome, and only a richer reason needs a
`CutShort`. Every contract-specific step of the loop — narrowing the final
round's offer, shaping its request, keeping only its terminal calls, the
nudge, the settling — is a method of the turn's `TurnPolicy`
(`openai_driver/turn_policy.rs`), so the loop itself never asks whether a
contract applies. `drive` takes the settled outcome (`AgentRun::take_outcome`); a
run whose turn panicked before `run_turn` returned was never settled and reads
as `failed` ("interrupted").

`RunOutcome` serialises as `{"status": "finished", "result": …}` /
`{"status": "incomplete", "reason": {"kind": …}, "summary": …}`, so a later
consumer can hand it back as data.

The schema is checked by a deliberately small validator: `type` (one name or a
list), `properties`, `required`, `enum`, `items`, and a boolean
`additionalProperties`. The annotations `title`, `description`, `default`,
`examples`, and `$schema` are ignored. Any other keyword (`pattern`, `oneOf`,
`$ref`, …) makes `FinishContract::new` fail, so a schema is never only partly
enforced. See [`dependencies.md`](dependencies.md) for why no validator crate
is used.

### Suspend and resume

A tool that needs a decision from outside the model returns
`aiplane_runtime::suspend::tool_suspend(SuspendRequest { kind, message,
timeout_secs, on_timeout })` rather than a result — an envelope with one
sentinel key, the same mechanism as `tool_content_parts`. `FeedbackHub` parks a
call only while the turn lives in memory; this pause is durable.

- **Pausing.** After the round's tools ran, the chat driver records every other
  call's result as usual, leaves the waiting call's row `running`, and writes a
  `chat_turn_suspensions` row (migration `0077_agent_builder.sql`): the call, the turn's round
  messages so far (`tail`), the budget spent (`{rounds, seconds, tokens}`), a
  fresh `request_id`, and the deadline. The turn becomes `suspended` in the
  same transaction, the worker ends without finalizing, and the stream emits
  `suspended` (see [`ui.md`](ui.md#chat-streaming-the-json-event-protocol)).
  One decision at a time: a second suspend request in the same round is
  answered with an error. Where the run cannot pause (`ToolContext::suspend`
  is `Unavailable`: `/v1`), the envelope is
  answered with an error too, and a well-behaved tool refuses on its own
  first. The timeout fallback is the kind's (`SuspensionKind::timeout_fallback`),
  whatever the tool asked: an approval always falls back to deny.
- **Resuming.** `suspend::claim_for_resume` checks the decision against the
  kind's options (`approval`: allow once / deny; `secure_input`,
  `human_answer`: value / deny) and the optional `request_id`, then deletes the
  row and flips the turn back to `in_progress` atomically, so a decision is
  claimed once. The chat path reserves the worker slot *before* claiming. The
  driver (`OpenAiDriver::resume`) rebuilds history and the system message as
  for any turn, appends `tail`, settles the waiting call and continues the
  round loop at the stored round count — and against the stored seconds and
  tokens, so the run's `Budget` covers it before and after the pause:
  - **deny** answers the call with a tool error naming why (`declined` by the
    user, or the request `expired`); the tool does not run;
  - **allow once** / **value** run the same call again with
    `ToolContext::suspend = Decided(decision)`, and its result is the call's
    result. Asking again re-suspends the turn.
- **Expiry.** `pages::chat::spawn_suspension_sweeper` looks every 30 s (and at
  boot) for suspensions past `expires_at` and resumes them with their
  `on_timeout` fallback, which is deny for every kind today (see
  `SuspensionKind::timeout_fallback`). Agent conversations are swept by
  `agents::resume::resume_expired` from the same loop.
- **Restart.** Nothing is held in memory: the startup sweep leaves suspended
  turns and their waiting call alone, and the resume runs on whichever
  process gets it.
- **Holding the conversation.** A suspended turn holds its conversation: a
  new message is refused with `409 decision_pending`
  (`agents::run::refuse_if_waiting`, the rule agent runs and the test chat
  follow too), since it would run before the paused call, in a context the
  decision was not asked about; `…/cancel` gives the decision up. The public
  embed endpoint alone queues one visitor message behind the decision.

`AskFirst::new(tool, timeout)` (`server/tools/ask_first.rs`) wraps a tool
in an approval: it keeps the wrapped tool's id and schema, pauses every call for an
`approval`, runs the tool only on `Decided(AllowOnce)`, and refuses where
pausing is impossible. Nothing in the shipped registry is wrapped; an agent's
spec wraps its own tools with `tool_resources.<tool>.permission: always_ask`,
and a tool whose `Tool::changes_state()` is true (an MCP tool marked
destructive and not read-only) asks by default
([`agent-hil.md`](agent-hil.md)). A person's connector tool in `ask`
mode is wrapped in chat ([`connectors.md`](connectors.md#tool-modes-always-ask-off)).
`schedule_action` and `delete_scheduled_action` check their arguments first
and then ask the same way, through `ask_first::approval` (the protocol
`AskFirst` runs on), with a preview of what they would do as the request's
`message`. Every approval in the product is this one durable pause; `ask_user`
and browser control keep `FeedbackHub` because they are answered within the
live turn, whose sandbox and browser leases a pause would end. Scheduled and webhook runs pause
like a chat, and their owner answers from the inbox.

The row has a `child_turn` column for a pause inside a sub-agent run, which
suspends every ancestor turn and resumes innermost first
([`agent-hil.md`](agent-hil.md#suspend-and-resume)). `forward_request`
sets it: a paused sub-agent run pauses its caller on the same request, and
one decision resumes the child first, then each caller with the child's
result (`ResumeFrom.child_result`). Agent runs pause and resume through
`agents::resume`, not the chat path's route.

### Streaming

When the client asked for `stream: true`:

- Each round streams the upstream SSE through live, but gateway-owned
  `tool_calls` deltas (and their `finish_reason: "tool_calls"` terminator) are
  suppressed — the client must not see calls it can't run. The accumulated
  calls execute server-side and the loop re-POSTs for the next round.
- The **final** round (the one producing no gateway-tool calls) streams
  straight through, terminator and all.
- **Mixed / client-owned turns** — the suppressed calls are re-materialised as
  one synthesized assistant delta plus a `finish_reason: "tool_calls"` chunk so
  the client receives the full turn, then the stream ends (`[DONE]`). Same
  yield-to-client rule as the buffered path.

## Returning images from a tool

A tool result is normally JSON, stringified into the `role:"tool"` message. For
image bytes that isn't enough — a tool result has no other channel for them. A
tool can instead return `tool_content_parts(vec![…])`, an envelope keyed by
`TOOL_CONTENT_PARTS_KEY`, and the driver emits `content: [ …parts… ]` (an array
of OpenAI content parts) rather than a JSON string. `fetch_attachment` and
`fetch_url` use it to hand a vision model a real `image_url` part.

The envelope is honoured only when the sentinel is the object's *sole* key, so
a tool that happens to carry a field of that name isn't misinterpreted.

## What the user sees

A caller with the `engineering` role POSTing to `/v1/chat/completions` gets a
normal OpenAI response. The model may have invoked tools zero or many times
along the way; the response carries only the final assistant message.

Audit records per round: user, model, tool ids invoked, arguments, latency,
success/failure.

## Intentionally out of scope

- **User-defined tools.** All tools are code-defined and reviewed.
- **Tool result caching.** Tools run every time they are called.
- **Model-driven agent orchestration on `/v1`.** A `/v1/chat/completions`
  caller still gets one tool loop and builds any orchestration of its own on
  its side. Gateway-defined agents and sub-agents are a separate surface, built
  on the headless runtime and the [finish contract](#finish-contract); their
  design is in [`agents.md`](agents.md).
