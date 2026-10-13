# Agents

People with the agent-management permission compose an **agent**: a
conversational main agent with typed state, hard gates, a router,
specialised sub-agents and people in the loop. They embed it on their own
website, where it talks to visitors who are never trusted, or serve it to
other agent platforms over A2A.

Organization Dots reuse the same principal, spec, model grants and share
editor, with an immutable organization owner and current group-derived
membership. Members with write access can manage their conversations,
automation, publication and the Dot’s own OAuth credential in the existing
Setup connection panel. Administrator configuration recovery alone supplies
no credential access. See [organization Dots](dots-organizations.md).

The `/agents` list can include personal and organization Dots alongside
ordinary agents. Dot cards identify the Dot type and active state, and omit the
generated principal ID; ordinary agent cards continue to show their authored
agent ID and publication state. The Dot owner and type come from the existing
Dot metadata returned with each agent, not from its principal name.

This file covers what an agent *is* — the trust rules, principals and grants,
the agent definition with its shares, models — and where the code lives. The
rest is split by topic:

| Doc | What it covers |
|---|---|
| [`agent-spec.md`](agent-spec.md) | The spec: layout, validation, the typed `AgentSpec`, state slots, gates, the topic scope |
| [`agent-runs.md`](agent-runs.md) | One run: where it lives, the system message, synthetic tools, the call chain, the router and route kinds (sub-agent, loop), bound arguments, the topic guard, the output filter, side calls |
| [`agent-hil.md`](agent-hil.md) | People in the loop: durable suspend and resume of agent runs, per-tool approval, hand-offs, the inbox, notifications |
| [`agent-visitors.md`](agent-visitors.md) | The public endpoint: embed keys, visitor sessions, the buffered stream, rates, the owner budget, retention, identity verifiers, voice |
| [`agent-a2a.md`](agent-a2a.md) | A2A in both directions: an agent served to other platforms, and an external agent as a route target |
| [`agent-activity-log.md`](agent-activity-log.md) | The tamper-evident activity log, and the analytics derived from it |
| [`agent-builder.md`](agent-builder.md) | What the builder runs on: the test chat, the resource list, evaluation, the setup assistant, the prompt assistant, the agent architect, draft revisions |
| [`embed.md`](embed.md) | For website owners: the widget snippet, theming, CSP |
| [`ui.md`](ui.md#agent-builder) | The SPA screens: workbench, setup, test chat, inbox |

## Trust rules

Every part of the agent system enforces these five rules. Code that breaks one
is a bug, even if it works.

1. **Security lives in gateway code, not in the model.** Gates, grants, bound
   arguments and verifier results are decided in Rust. An LLM may only *deny*,
   never *grant*.
2. **The model never picks the subject.** Whose data a tool touches comes from
   verified state, bound into the call by the gateway and hidden from the schema
   the model sees.
3. **Sub-agents get a task, not the transcript.** Their input is a template
   rendered from state. Their output is a schema-checked `finish` result that
   returns to the main agent as data.
4. **No privilege escalation.** A system principal starts with nothing. A
   manager can only grant what they hold at grant time.
5. **Enforce at the tool layer.** Every tool call is checked against the acting
   principal's grants, including calls inside nested sub-agents, and audited
   with the full call chain.

## Concepts

| Term | Meaning |
|---|---|
| **System principal** | An identity that is not a person: an agent, a CI job, an integration. Holds exactly its grants |
| **Agent** | A system principal plus a spec: a mutable draft and immutable published versions |
| **Organization Dot** | A named agent with an immutable organization owner, independent active-slot accounting and access narrowed by current membership and existing shares |
| **Manager** | A person whose groups have `can_manage_agents` (admins have it), acting on an agent through a share |
| **Share** | `respond` < `read` < `write` on one agent, held by a user or a group |
| **Conversation** | A `chat_sessions` row owned by the agent's principal; a visitor's, an A2A context, or a test conversation (version 0) |
| **Run** | One turn of a conversation, driven as the agent's principal; a sub-agent's run is a child session under the calling turn |
| **Visitor** | An anonymous person on a website that embeds the agent. Not a principal: a session under the agent |
| **Slot** | A typed piece of conversation state with a provenance (`llm`, `verifier:<id>`, `host`) |
| **Gate** | A route's condition over slots, decided in code |
| **Route** | A gate plus a target: a sub-agent, a person, an external A2A agent, or a worker/critic loop |
| **Suspension** | A durable pause of a turn waiting for an approval, a secure input or a person's answer |
| **Activity log** | The hash-chained record of everything an agent does (`agent_audit`) |

## Principals

Agents run as **named system principals**, never on a user's token. A new
principal has **no rights**: tools, connectors, skills, RAG collections and
models are granted one by one, and nothing company-wide is inherited. Only
holders of the agent-management permission create or configure principals.
Sub-agents are agents, each its own principal with its own grants. A visitor
is not a principal.

### Why a separate table, not a `kind` column on `users`

Three behaviours of a person's identity would make it unsafe for an agent:
- `Resolver::role_ids_for` puts **every `is_default` group** in front of the
  mapped ones.
- `Resolver::resource_allowed` treats an **empty `allowed_groups` as
  "everyone"**, which covers pools (and so the models they serve), RAG
  collections and MCP connectors.
- The MCP manager, memory, personal skills and per-user tool prefs are keyed
  by `user_id`.

A `users` row with `kind = 'agent'` would keep every `user_id` foreign key
working, and feed agents into all three inheritance paths, plus the OIDC
upsert by `sub`, admin user lists and impersonation. Each would need a guard,
and a missed guard is a silent grant. A separate table makes the compiler
find every site.

### Tables

```sql
CREATE TABLE system_principals (
    id          TEXT PRIMARY KEY NOT NULL,   -- uuid v4
    name        TEXT NOT NULL UNIQUE,        -- slug, shown in usage and audit: "support-website"
    display     TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_by  TEXT NOT NULL,               -- users.id
    created_at  TEXT NOT NULL,
    disabled_at TEXT                         -- disables every token and embed key
) STRICT;

CREATE TABLE principal_grants (
    principal_id TEXT NOT NULL REFERENCES system_principals(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('tool', 'connector', 'skill',
                     'rag_collection', 'model', 'a2a_caller', 'a2a_agent')),
    ref          TEXT NOT NULL,              -- tool id, connector key, skill, collection id,
                                             -- model name, agent id, agent card URL
    granted_by   TEXT NOT NULL,
    granted_at   TEXT NOT NULL,
    pools        TEXT,                       -- a model grant's pools; NULL = every serving pool
    PRIMARY KEY (principal_id, kind, ref)
) STRICT;

CREATE TABLE system_tokens (                 -- the standalone use: CI, integrations, A2A callers
    id           TEXT PRIMARY KEY NOT NULL,
    principal_id TEXT NOT NULL REFERENCES system_principals(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    hash         TEXT NOT NULL UNIQUE,       -- SHA-256 of the bearer, like tokens.hash
    created_by   TEXT NOT NULL,              -- the minter: caps the token (auth.md)
    created_at   TEXT NOT NULL,
    last_used_at TEXT,
    expires_at   TEXT NOT NULL,
    revoked_at   TEXT
) STRICT;

ALTER TABLE gateway_groups ADD COLUMN can_manage_agents INTEGER NOT NULL DEFAULT 0;
```

All agent tables are in the one migration `0077_agent_builder.sql`;
`crates/aiplane-core/migrations/README.md` explains why migrations run with
foreign keys off (seven tables cascade from `chat_sessions`).

- **Tokens.** System tokens have their own prefix, `gws_`; user tokens are
  `gwk_`. `require_bearer` routes by prefix, so a user-token lookup can never
  return a principal and the other way round. Token handling, the minter cap
  and the management routes are in [`auth.md`](auth.md#system-principals-and-gws_-tokens).
- **`can_manage_agents`** works like `is_admin` on a gateway group: a column,
  checked by `Resolver::can_manage_agents(role_ids)`. `is_admin` implies it.
  `GET /api/v0/me` reports it, and the SPA shows the Agents section only
  when it is true. Operators set it per group with the *Agent manager* switch
  on `/admin/groups` (`PUT /api/v0/admin/groups`, `can_manage_agents`; a body
  that omits the field leaves it unchanged). An admin group shows it held.

### Grants

What each grant kind unlocks, and what it does not, is the table in
[`tools-rbac.md`](tools-rbac.md#system-principals). `a2a_caller` and
`a2a_agent` are in [`agent-a2a.md`](agent-a2a.md).

- **Grant-time cap.** Writing a grant runs the manager's own check for that
  resource: `allowed_tools` for tools, `resource_allowed` for collections and
  connectors, `allowed_skills`, and for a model the list the manager's own
  chat picker is built from (`server::model_choices::offered`, see
  [Models](#models)). A refused grant is `403 grant_exceeds_manager`, naming
  the resource.
- **Grants persist.** After the grant is written it belongs to the principal
  and is never re-derived from the manager, so it survives the manager losing
  rights. A `gws_` token a non-admin minted carries only what the minter
  holds at each request ([`auth.md`](auth.md#management-api)).
- **Grants are not versioned.** They belong to the principal, not to a spec
  version. A live spec that names a tool whose grant was revoked hits default
  deny at run time: the tool is not offered and a call to it is refused as
  `not_granted`.
- **Who manages a principal.** An agent's principal: whoever holds a share on
  the agent ([Shares](#shares)). Any other principal: its creator
  (`created_by`) and admins; another manager gets 404. Issuing a token takes
  the cap for every grant at once (`403 token_exceeds_manager`), because the
  token hands them all out.
- **Every grant change is audited** in the activity log's agent chain, with
  `actor_id` the user who made it, on the change's own transaction.

### The `Principal` type

`aiplane-core` (`server/principal.rs`), identity only:

```rust
pub enum Principal {
    User { id: String, roles: Vec<String> },
    System(SystemPrincipal),
}

pub struct SystemPrincipal {
    pub id: String,
    pub name: String,           // the slug shown in usage and audit
    pub grants: Arc<GrantSet>,  // loaded from principal_grants, with each model grant's pools
}
```

`Principal::user_id()` is `Some` only for a person. Tools that act for a
person (memory, `notify_user`, `schedule_action`, location, browser control)
refuse with a clear error when it is `None`. `subject_id()` is the stable id
for scoping rows and attribution (`users.id` or `system_principals.id`), and
`kind()` is `user` or `system`. `ToolContext.principal` carries it; there is
no separate `user_id`/`roles` on the context, so every place identity matters
asks the principal.

### How each subsystem treats a system principal

| Subsystem | `Principal::User` | `Principal::System` |
|---|---|---|
| Tool offer (`AppState::allowed_tools_for_*`) | groups ∩ registry, per-conversation narrowing | `grants[tool]` ∩ registry, plus a run's synthetic tools ([`agent-runs.md`](agent-runs.md#synthetic-tools)). No `enable_tools` bootstrap and no `"*"` |
| Skills (`allowed_skills_for`, `read_skill`) | groups plus overlay | `grants[skill]` only |
| MCP (`McpManager::layer_for_user`) | the user's connected per-user connectors plus global ones | `grants[connector]`, restricted to connectors with scope `global` or `agent`. `user_mcp` is never read |
| RAG collections | `resource_allowed` | `grants[rag_collection]`. An empty `allowed_groups` does **not** count |
| Models (`PoolAccess`) | pool `allowed_groups` plus the token allowlist | `grants[model]` only, each through its recorded pools ([Models](#models)) |
| Memory, personal skills, user tool prefs | yes | no access: `user_id()` is `None` |
| Usage | `usage_events.user_id` | `principal_kind = 'system'`, the principal id in `user_id`; in a run also `agent_id` and `chain` |
| Limits | subject `user`/`role`/`global` | subject `system`: an operator's cap on the agent, next to the owner's `publish.budget` ([`agent-visitors.md`](agent-visitors.md#owner-budget)) |
| Audit | `mcp_tool_audit.user_id` | plus `principal_kind` and `chain` |

- **Connector scope `agent`.** It sits next to `per_user` and `global` in
  `mcp_catalog`. Such a connector is invisible to every person and usable only
  by principals granted it. This is how an owner wires their own ERP or
  ticket MCP, with a static bearer, without exposing it to employees. A
  `global` connector cannot serve this: with empty `allowed_groups` it is open
  to everyone.
- **Usage columns keep their names.** `user_id` already means "who is
  billed"; adding `principal_kind` keeps every existing query correct for
  users.

## Agent definition

An agent is a system principal plus a spec. The spec's layout and validation
are in [`agent-spec.md`](agent-spec.md).

```sql
CREATE TABLE agents (
    principal_id  TEXT PRIMARY KEY NOT NULL REFERENCES system_principals(id) ON DELETE CASCADE,
    draft_spec    TEXT NOT NULL,              -- JSON, freely editable
    live_version  INTEGER,                    -- NULL = never published
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
) STRICT;

CREATE TABLE agent_versions (
    principal_id  TEXT NOT NULL REFERENCES agents(principal_id) ON DELETE CASCADE,
    version       INTEGER NOT NULL,
    spec          TEXT NOT NULL,              -- immutable snapshot
    published_by  TEXT NOT NULL,
    published_at  TEXT NOT NULL,
    PRIMARY KEY (principal_id, version)
) STRICT;

CREATE TABLE agent_shares (
    principal_id  TEXT NOT NULL REFERENCES agents(principal_id) ON DELETE CASCADE,
    subject_kind  TEXT NOT NULL CHECK (subject_kind IN ('user', 'group')),
    subject_id    TEXT NOT NULL,
    access        TEXT NOT NULL CHECK (access IN ('respond', 'read', 'write')),
    PRIMARY KEY (principal_id, subject_kind, subject_id)
) STRICT;
```

Rows live in `aiplane-agents::db::agents`; every mutation writes an activity
log event on the same transaction (`agent_created`, `agent_draft_updated`,
`agent_published`, `agent_live_version_set`, `agent_share_set`,
`agent_share_removed`, `agent_deleted`).

- **Keyed by the principal.** Creating an agent creates its system principal,
  with no grants, in the same transaction. Deleting an agent deletes the
  principal, which cascades to its grants, tokens, versions and shares; the
  agent's activity log stays. Grants are made through
  `/api/v0/system-principals/{id}/grants`, which for an agent's principal also
  require a share: `read` to see it, `write` to change grants or tokens or to
  disable it. The principal list hides agents the caller holds no share on.
- **Drafts and versions.** Edits go to `draft_spec`. Publishing validates the
  draft, snapshots it as version `n+1` and moves `live_version`. Rolling back
  (or forward) sets `live_version` to another snapshot without re-validating,
  since grants are not versioned. Visitors, A2A callers and parent agents run
  published versions; a draft runs only in the builder's test chat, an
  evaluation and the architect's test turn ([`agent-builder.md`](agent-builder.md#test-chat)).
  Every draft save keeps the draft it replaced as a revision
  ([`agent-builder.md`](agent-builder.md#draft-revisions)).
- **A conversation is pinned to its version.** It runs the version that was
  live when it started (`chat_sessions.agent_version`); publishing or rolling
  back changes only conversations started afterwards. Versions are immutable
  and deleted only with the agent, so a pinned version always exists. Limits,
  budget, retention and the A2A opt-in are the exception: they are read from
  the **live** version, so lowering one applies to every open conversation.

### Shares

- **Levels.** `respond` < `read` < `write` (`agents::Access`), each including
  the ones below. One table (`agent_shares`), one matcher (`holder_clause`),
  one rule: `aiplane_runtime::agents::access::effective_access`. It answers
  `write` for an admin; otherwise the strongest share, but `read` and `write`
  only while the person holds `can_manage_agents`; otherwise nothing. The
  `/api/v0/agents` routes, the inbox's standing and the `a2a_caller` grant
  cap all ask it, so a manager who loses the permission loses every agent
  with it, whatever shares are left behind.
- **`read`** shows the spec, the activity log and the analytics. It needs
  the permission too, because the agent's conversations hold visitor data.
- **`write`** edits, publishes, shares, grants and deletes. The creator gets a
  `write` share automatically. Removing or downgrading the last `write` share
  is refused (`409 last_writer`). Admins implicitly hold `write` on every
  agent, so an agent whose last writer left stays recoverable.
- **`respond`** needs no permission: support staff who answer approvals and
  hand-offs in the inbox ([`agent-hil.md`](agent-hil.md#the-inbox)). It is the
  only level that holds without the permission, and only when held as
  `respond` itself (a manager's `write` share does not degrade to `respond`
  when they lose the permission). Every other agent route treats it as no
  access (`404`), and `GET /api/v0/agents` does not list the agent.
- **Without a share an agent answers 404**, not 403.
- **A share names a subject that exists**: a user id from `users`, or a group
  the RBAC resolver knows (`Resolver::has_group`: database groups, which the
  `[rbac]` config seeds, and the bootstrap admin group). For `read` and
  `write` the holder must have `can_manage_agents` when the share is written:
  a user through their groups, a group through the flag or `is_admin`, asked
  of the resolver; otherwise `422 share_needs_agent_manager`, naming
  `respond` as what can still be given.
- **The sharing panel picks subjects from a search, never a roster.**
  `GET /api/v0/agents/{id}/share-subjects?q=` (`write`). *Chosen (privacy):*
  holding `can_manage_agents` must not hand anyone the people directory, which
  only admins list (`/api/v0/admin/users`). So it answers nothing for a query
  under two characters, at most eight users and eight groups (name or address
  contains it, case-insensitive; `users::search`), a user as id and display
  name with the address only when the query is exactly it, and no hint
  whether a subject holds the agent-management permission. A share in the
  list carries the person's display name.

### API

`aiplane-api::pages::json_agents`. Every route needs a session with
`can_manage_agents`; the share column is what the caller must hold.

| Method | Path | Share | Purpose |
|---|---|---|---|
| GET | `/api/v0/agents` | any | Agents shared with you `read` or `write` (every agent for an admin), with your `access` |
| POST | `/api/v0/agents` | — | Create `{name, display?, description?, spec?}`; 201, 409 on a taken principal name |
| GET | `/api/v0/agents/{id}` | read | `{agent}`: the agent, `draft_spec`, `live_spec`, `publish_issues`, `limits` ([`agent-visitors.md`](agent-visitors.md#owner-budget)), grants, shares, the decision trail |
| PUT | `/api/v0/agents/{id}/draft` | write | Replace the draft `{spec}`; the live version is untouched |
| POST | `/api/v0/agents/{id}/draft/restore` | write | `{revision}` ([`agent-builder.md`](agent-builder.md#draft-revisions)) |
| POST | `/api/v0/agents/{id}/publish` | write | Validate the draft for publishing, snapshot it as version n+1, make it live |
| GET | `/api/v0/agents/{id}/versions` | read | Every version, newest first, with its spec |
| POST | `/api/v0/agents/{id}/live` | write | `{version}`: roll back or forward |
| GET | `/api/v0/agents/{id}/shares` | read | The shares |
| POST | `/api/v0/agents/{id}/shares` | write | `{subject_kind: user\|group, subject_id, access: respond\|read\|write}`; 404 for an unknown subject, `422 share_needs_agent_manager` |
| POST | `/api/v0/agents/{id}/shares/revoke` | write | `{subject_kind, subject_id}` |
| GET | `/api/v0/agents/{id}/share-subjects?q=` | write | `{users: [{id, name, email?}], groups: [name]}` |
| DELETE | `/api/v0/agents/{id}` | write | Delete the agent and its principal |

An invalid spec is `422` with `error.code = "invalid_agent_spec"`. The
`message` names the first problem; `error.issues` lists all of them as
`{path, message}`. Each ungranted reference says which grant to make, for
example `POST /api/v0/system-principals/{id}/grants {"kind": "tool", "ref":
"rag_search"}`.

The other agent routes are documented with their subject: the test chat,
resources, tests, assistant and architect in
[`agent-builder.md`](agent-builder.md), resume, inbox and channels in
[`agent-hil.md`](agent-hil.md), embed keys and `/api/v0/embed/*` in
[`agent-visitors.md`](agent-visitors.md), activity and analytics in
[`agent-activity-log.md`](agent-activity-log.md), `/a2a/*` in
[`agent-a2a.md`](agent-a2a.md).

## Models

An agent names models the way a person picks one in the chat: a model id, a
backend alias or an automatic-route alias. The model keys are `main.model`,
`router.model`, `scope.classifier_model` and
`publish.voice.transcription_model` / `speech_model`. A sub-agent runs on its
own spec's `main.model`; the evaluation's rubric judge on the agent's main
model. There are no agent-specific model settings: an agent uses the chat's
models, aliases and automatic routes.

**One list** (`aiplane-runtime::server::model_choices`). `offered(state,
kind, access)` is what a caller with `access` may pick of a kind (chat,
transcription, speech): the registry's models and aliases of that kind
(`models_with_compliance_for_kind_for`, then the access's model check), plus
— for chat — every automatic route whose alias the caller may name and whose
fallback they may reach, sorted, the gateway default first.
`GET /api/v0/models` (the chat picker), `GET /api/v0/transcription_models`,
`GET /api/v0/agent-resources`, the grant cap (`grant_holding::holds` for
kind `model`) and the prompt assistant's choice all read it. An automatic
route carries `RouteChoice { candidates, whole }`: `whole` when the caller may
also use every candidate and the selector. The chat picker lists a route
whose fallback is reachable; a manager may **grant** one only when it is
whole (`ModelChoice::grantable`), because the grant hands the agent every
model the route can send to.

**Unset means the gateway default.** `gateway_default(state, feature)` is the
admin's *Default models* choice (`/admin/models`, `app_settings`
`default_model.{chat,transcription,speech}`) when the gateway offers it, else
the first model it offers — `feature_defaults::resolve`, over the whole
gateway, not over the agent's grants. `agents::defaults::{main_model,
voice_model}` apply it at run time; `SpecContext::model_defaults` carries it
to the validator, which on publish requires the default an unset key runs on
to exist and be granted. So an admin changing the default moves every agent
that names none, and an agent not granted the new default stops with
`ModelNotGranted` (`422 agent_model_not_granted` in the test chat, `503` for
voice) until it is granted or names a model.

**Pools of a grant.** A `model` grant stores, in `principal_grants.pools`,
every pool of the model's kind the granting manager may use — for an
automatic route, of chat and selector models
(`grant_holding::model_grant_pools`, `UpstreamRegistry::pools_of_kinds`).
That is decided by the manager's groups, not by what is serving at grant
time: a pool that is down or not probed yet still counts once it serves the
model, and routing takes the recorded pools that serve it at request time.
The principal routes the name only through those pools, as a person's chat
stays inside their groups' pools. An admin's grant stores `NULL`: every pool
serving it. **A regrant never narrows**: the pools become the union of what
the grant held and the new grantor's pools (`NULL` wins), the first grantor
stays, and `POST …/grants` answers `{added, widened}` (`201` when new, `200`
otherwise, `widened` when it reaches more pools). Taking a pool away is a
revoke and a new grant.

**A token narrows it again.** A `gws_` token minted by a non-admin carries
each model grant only through the pools of the model's kind its minter may
use *now*, intersected with the grant's (`grant_holding::capped_to_minter`);
an empty intersection drops the grant. An admin's token keeps the grant's
pools.

**Access** (`PoolAccess::granted_models`). A system principal's access is
its `model` grants, each with its pools: the grant replaces the pools' group
rule, and a name routes only through its pools (`PoolAccess::reaches`). A
granted backend alias authorises what a backend of one of its pools resolves
it to, there and nowhere else — but only for the gateway's own resolution of
a name the caller was allowed (`PoolAccess::resolving` / `for_request`): the
turn resolves `fast` to the real id before it routes, the `/v1` chat and
messages paths resolve the requested name up front, and the conversation's
compaction routes the resolved id too. A caller naming the target itself
(`Qwen/Qwen3` with only `fast` granted) holds no grant on it: `404`, and
`GET /v1/models/{id}` does not know it. Default deny: no `model` grant, no
model.

- **An agent run is narrowed to what its spec names.** A run reaches only
  models both granted and named by its spec, or the gateway default it runs
  on: `PoolAccess::for_system_models(principal, listed)`. That covers the
  turn's rounds, the router's classifier, the topic guard, the conversation's
  compaction summary, the rubric judge and voice. Tools that call a model
  themselves (image generation) keep the principal's model grants: the tool
  grant is what allows them.
- **Automatic routes** are resolved like a chat turn
  (`server::model_route::route_target` → `AutomaticRouter::select`): the
  selector and candidates are reached under the access widened by the route's
  members (`PoolAccess::for_route_targets` over `AutomaticRoute::members`,
  each through the route grant's pools), and the chosen target is routed
  under the access widened by exactly that target; a candidate that is
  itself an alias reaches its target the same way. Compaction compacts on
  the target the route pinned for the session, else its fallback
  (`compaction::compaction_target`).
- **Fallback.** The kind's unknown-model fallback (`[fallback].<kind>`)
  applies to an agent only when it is granted as well, and to a person's
  token only when its allowlist lets the fallback through.

The builder offers one picker per model key over this list
([`ui.md`](ui.md#agent-setup)): choosing a model stages its grant; choosing
*Default* stages the default's grant when the manager may give it.

Tests: `server::model_choices`, `upstreams::registry`
(`a_system_principal_reaches_only_its_granted_models_whatever_the_pool_groups`,
`an_agent_run_reaches_only_models_both_granted_and_listed`,
`an_automatic_route_target_reaches_only_the_routes_pools`),
`spec.rs`, `agents/run/tests.rs`, `tests/it/automatic_routing.rs`,
`tests/it/system_principals.rs`, `tests/it/embed/voice.rs`,
`web/src/lib/agent-setup.test.ts`.

## Shared mechanisms

Work on agents uses these; a second mechanism for the same purpose needs the
maintainer's approval (AGENTS.md rule 11). The architecture tests
([`testing.md`](testing.md#architecture-tests)) enforce several of them.

| Purpose | Mechanism |
|---|---|
| Who may act on an agent | `agents::access::effective_access` over `agent_shares` |
| Reading a spec | the typed `AgentSpec` from `CompiledSpec::agent()`; only the validator walks the JSON |
| Validating a spec | `spec::check` / `spec::validate` with a `SpecContext` (`SpecWorld`) |
| Writing a slot the model cannot | `state::write_trusted` with a `TrustedWriter` |
| Which tools a run may call | `GrantedToolSource`, with `RunToolSource` over it |
| Model lists and grants | `server::model_choices`, `grant_holding` |
| Running an agent turn | `drive_opened` / `drive_opened_from` behind the installed `AgentTurnRunner` |
| One turn at a time per conversation | `agents::embed::claim` on the session worker registry |
| Pausing for a decision | `chat_turn_suspensions` through `suspend::tool_suspend`; `agents::resume` for agent runs |
| A one-off model call | `server::side_call` (`ask_text`, `ask_json`) |
| Rates | `rates::record_within` over `rate_events`, in one `WriteTx` |
| Recording what an agent did | `agent_audit::append`, reached through `agents::audit` |
| Secrets in a spec | `spec::secrets::seal_spec_secrets` (`*_sealed` keys) |
| Outbound URLs an owner chose | `outbound_guard` with `Policy::agent` |
| Reading a body | `capped_read::read_capped_for`, `read_body_capped` / `read_json_capped` |
| What a resource is called | `tool_toggles::CapabilityEntry` (the chat picker's rows) |

## Crate placement

The rule from `AGENTS.md`: put code as high as it will go, and never reference
upward.

| Piece | Crate | Why there |
|---|---|---|
| Migrations (one set, agent tables included); the `can_manage_agents` resolver check; `Principal`, `GrantSet`, `RunChain`; the `agent_id` column of usage and the per-agent spend limits | `aiplane-core` | the migration history is never split; identity types are read by RBAC, the upstream registry and usage metering, all below the features |
| db modules of every agent table; principal-owned conversations, the agent pause sweep and the inbox reads (`db::run_sessions`); the visitor rate gate and the rate primitive (`rates`); the inbox webhooks (`notify_channels`); the activity log (`db::agent_audit`) | `aiplane-agents` | nothing below the runtime reads them, so they sit on `aiplane-core` beside `aiplane-features`: an agent DB edit does not rebuild the base layer, and a runtime edit does not recompile them |
| `chat_turn_suspensions` db functions; the `suspended` status and `chat_json` event | `session-core` | the chat substrate owns turn lifecycle and the SSE protocol; it reads a conversation by `user_id` and treats any other owner as opaque |
| Spec types and validation, the gate evaluator, the schema-subset validator, template rendering | `aiplane-runtime` (`agents/`) | the lowest crate that needs them at run time; `aiplane-api` validates on save through it |
| `ToolContext.principal`, `AgentRun`, `RunProfile`, finish/budget/suspend, the run's tool source (synthetic tools, router, dispatch, loop), output filter, injection scan, principal-aware tool/skill/MCP/model resolution, `model_choices`, `model_route`, `side_call` | `aiplane-runtime` | they are the loop and the tool machinery; every consumer of the model list is here or above |
| Verifier tools (`mcp_code`, `lookup`), host JWT | `aiplane-runtime` (`agents/verifier/`) | run-scoped synthetic tools like `set_<slot>`, built from the spec and writing through `TrustedWriter`; `aiplane-tools` cannot be reached from the run |
| The A2A client (guard, card cache, exchange); the A2A card and state mapping | `aiplane-runtime` (`agents::a2a_client`, `agents::a2a`); the waiting task's row in `aiplane-agents` | `forward_request` dispatches an A2A route like a sub-agent |
| The prompt assistant (`agents::assist`) and the architect's persona hook (`persona`, `TurnPolicy::Persona`, `assist::apply_changes`) | `aiplane-runtime` | the review needs the validator and the test-case parser; the driver only knows "a prompt and a tool source" |
| `/api/v0/agents/*`, `/api/v0/system-principals/*`, the inbox, resume, test chat, tests, activity, analytics, assistant, architect (its tools and `draft/restore`), `/api/v0/embed/*`, `/a2a/agents/*` | `aiplane-api` | JSON handlers; the architect's tools need the route functions (share checks, grant cap, test chat), so the API builds the persona per turn and hands it down |
| Routing, the embed CORS layer (`rama_server::embed_cors`), `gws_`/`gwv_` bearer dispatch | `gateway` | routing glue; the embed CORS layer reads embed keys, so it cannot sit in `aiplane-core` beside the `/v1` one |
| Builder UI, test chat, inbox | `web/` (SPA) | daisyUI + Tailwind, all strings through Fluent |
| Embed widget | `web/embed/`, its own Vite entry built to `target/frontend/build/embed.js` | must not pull in the SPA; strings come from the shared catalogs |

No agent feature needs a Cargo dependency of its own. Host-JWT verification
uses `jsonwebtoken`, patterns use `regex`, hashing uses the token helpers.
