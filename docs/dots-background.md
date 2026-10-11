# Personal Dot background execution contract

Scheduled actions and webhooks support an optional personal Dot binding. They
use AIplane's existing background execution, chat, approval and user-budget
paths. [Organization schedules](dots-organizations.md) and
[organization webhooks](dots-webhooks.md) use the shared principal admission path.
[Separate connector identities](dots-identities.md) preserve their selected owner and credential generation.

## Execution context

Trigger configurations belong to a user. Their workers open user-owned,
private conversations and load the same personal Dot persona, memory, selected
tools and approval rules used by interactive turns. The user's security
principal and spend limits apply to every run.

Run history stores the exact assistant turn ID together with the tool toggle
and history limit used at startup. Approval continuation retains its original
`Scheduled` or `Webhook` usage source, intersects the recorded tool toggle with
the current configuration, and reconciles the exact run's outcome. A
continuation of an older run preserves the summary of a newer fire.

Dot bindings survive Dot deletion. Missing or inactive Dots refuse execution.
Trigger deletion is refused while a correlated turn is running or suspended,
and while startup has not yet attached a turn. Disabling a trigger remains
available.

## Shared mechanisms

| Concern | Mechanism |
|---|---|
| User-owned trigger configuration | Existing `scheduled_actions`, `webhooks`, CRUD handlers and forms |
| Cron evaluation and dispatch | `server::scheduled::{cron,worker}` |
| Secret webhook URLs and bounded untrusted input | Existing `/hooks/{secret}`, token hashing, `read_body_prefix`, untrusted payload envelope |
| User-owned sessions and turns | `headless::open_session`, `session_core::db` |
| Live background output, cancellation and shutdown | `SessionWorkers::register_background`, the existing per-worker broadcast and JSON-SSE stream; background workers share session exclusivity but do not consume interactive capacity |
| Dot attribution and binding | `Actor::personal_dot`, `aiplane_agents::db::dot_sessions` |
| Dot memory and tool policy | The interactive personal Dot persona loader, moved into runtime for both callers |
| Tool resolution and approvals | `GrantedToolSource`, live user grants and MCP overlay, typed `AgentSpec`, `AskFirst`, `chat_turn_suspensions` |
| Approval continuation | Existing chat/Inbox claim paths and `headless::drive_resumed` |
| Run history and outcome | Existing scheduled/webhook run rows and terminal `TurnStatus` |
| Usage and quotas | Existing `UsageRecord`, `UsageSource::{Scheduled,Webhook}`, owner limits |
| UI selection | Existing Dot list data, schedule/webhook forms, `LinkedChatPicker`, run-history pages |
| Scoped conversation search | `session_core::db::search_sessions_scoped`, with an owner-agnostic set of candidate session IDs applied before title/content limits |

The shared extensions have real callers in this package: a runtime personal
Dot-context loader used by interactive and background turns, an exact
background-turn lookup/reconciliation path used by initial execution, resume
and cancellation, and common session admission and worker capacity handling.
Their state-based tests cover real SQLite storage and mocked upstreams.
No new task queue, trigger engine, approval table or billing engine is needed.

`SessionWorkers` distinguishes interactive and background capacity within its
existing entries. `running_for_user` counts interactive workers;
`sessions_for_user`, `active_count`, cancellation, subscriptions and shutdown
include both. Background turns use the same per-session worker and broadcast,
so an owner can watch and stop the run through the existing chat surface.
Interactive messages cannot steer or rewrite a running background turn.
`create_turn_pair_if_idle` holds the SQLite write lock while checking active
and suspended turns and inserting both rows. Background callers also reject
queued user messages. `regenerate_turn_if_idle` uses the same admission lock
to validate the actual target, edit or remove history and insert its replacement
answer atomically. Retry/edit use the existing attachment reclamation wrapper
and reserve the existing worker before this transaction; a competing background
opening cannot delete or replace their execution context.
The existing pending-chat dispatcher excludes every busy session while using
only interactive workers for its capacity check.

## Schema amendments

Amend the unreleased `0081_personal_dot_sessions.sql` change set, its schema
fixture and checksum. Do not add a migration per work package.

1. Keep `personal_dot_sessions.session_id` and `owner_user_id` foreign-key
   cascades. Remove the Dot foreign key from `dot_id`; retain the non-null Dot
   ID as a tombstone. A left join against an owner-matched `personal_dots` row
   distinguishes a live binding from an unavailable Dot. Missing or inactive
   Dots cannot become ordinary chat.
2. Add nullable `dot_id TEXT` to `scheduled_actions` and `webhooks`, without a
   Dot foreign key. `NULL` explicitly means ordinary user execution. A
   non-null missing ID means unavailable Dot execution, which is refused.
3. Add these fields to both existing run-history tables:

   ```sql
   assistant_turn_id TEXT,
   tools_enabled INTEGER NOT NULL DEFAULT 0 CHECK (tools_enabled IN (0, 1)),
   history_limit INTEGER CHECK (history_limit IS NULL OR history_limit >= 0)
   ```

   Create a unique partial index on `assistant_turn_id` in each table where
   it is non-null. The turn ID remains a historical string if its chat is
   deleted. Existing terminal history can have a null turn ID; every newly
   started run must persist its exact turn ID before model execution.

`tools_enabled` and `history_limit` are execution snapshots. Source is inferred
from the run table, rather than duplicated in a second policy table. The
loader must reject multiple matching runs across the two tables. It must
return database failures as failures, never interpret them as interactive
chat. Existing run-to-trigger foreign keys remain; the deletion guard below
protects every nonterminal correlated turn before those rows can disappear.

Extend existing `NewAction`, `EditAction`, `ScheduledAction`, `NewWebhook`,
`EditWebhook` and `Webhook` with `dot_id: Option<String>`. Extend scheduled
`attach_run_session` into an exact `attach_run_turn` operation taking run ID,
session ID, assistant turn ID, tools snapshot and history cap. Extend webhook
`record_run_start` with the same turn/snapshot fields. These write operations
must validate source owner and session ownership and write atomically.
Deletion returns a typed deleted/missing/pending-run result so API callers can
map pending execution to `409`.

## DB and API contracts

Extend the existing schedule and webhook create/update bodies with
`dot_id: string | null`. Their full create/update forms send this field;
an omitted field means ordinary user execution. Existing linked-session
semantics remain: omitted leaves the pointer unchanged, an empty string
requests a fresh next chat, and a concrete ID requests reuse.

Row responses add `dot_id: string | null` and
`dot: { id, name, display, active } | null`, sourced from the owned Dot row.
A non-null `dot_id` with `dot: null` exposes an unavailable binding, not an
invented Dot name. Existing list responses also include the caller's owned
Dot choices. This does not require permission to create new Dots: an existing
owner may configure their existing resources. Creation still uses the current
Dot creation permission.

Create and changed-binding validation must verify that a selected Dot belongs
to the trigger owner and is active. An unchanged inactive or deleted binding
may remain while unrelated configuration is edited; it remains unavailable
for execution. Persist the ownership/active check and binding change
transactionally. Changing `dot_id` clears `last_session_id`, unless a
new explicitly supplied session passes the new binding validation.

A reusable linked session must be private, user-owned and have exactly the
requested binding: the same personal Dot, or no Dot for ordinary execution.
Another Dot's chat, an unavailable Dot binding, a shared chat or another
user's chat is rejected. A deleted previous chat permits a fresh one; a
database error must not silently select a different execution context.

Extend the existing chat-session list route with an optional `dot_id` query:
absent preserves the existing all-session listing, an empty value selects
ordinary sessions, and a concrete value selects only that owner's sessions
bound to that Dot. Filtering belongs in the API/agent layer. Do not add Dot
concepts to the owner-agnostic `session-core::Session`.
When the existing `q` search parameter is present, apply the same binding
filter to search results; search cannot bypass the linked-picker scope.

Existing run-history responses add `assistant_turn_id`; existing session
links and deleted-chat indicators remain authoritative. CRUD errors use
localized messages. Owner-facing invalid binding is `400`; unavailable or
inactive Dot execution is `409`. Anonymous webhook triggers keep the existing
indistinguishable not-found behavior for unavailable bindings.

## Initial run and continuation

The coordinator adopted these shared caller contracts:

```rust
persona::personal_dot::context_for(
    state: &RamaState,
    user: &User,
    session_id: &str,
    tools_enabled: bool,
) -> Result<Option<PersonalDotContext>, DbError>

server::background::for_turn(
    db: &Pool,
    user_id: &str,
    assistant_turn_id: &str,
) -> Result<Option<BackgroundTurn>, DbError>

server::background::reconcile_turn(
    db: &Pool,
    user_id: &str,
    assistant_turn_id: &str,
) -> Result<(), DbError>
```

`PersonalDotContext` is an enum:
`Active { actor: Actor, persona: Arc<ChatPersona> }` or `Unavailable`.
`None` means explicit ordinary session only. Initial/background resume callers
refuse `Unavailable` before a model request, rather than merely offer a model
an empty refusal persona.
`BackgroundTurn` carries `source: UsageSource`, `history_limit: Option<usize>`
and the effective `tools_enabled: bool`. It resolves the current source row
and compares its owner/Dot binding with the bound session. A Scheduled/Webhook
headless call without durable correlation is refused. Uncorrelated ordinary
headless test/manual callers do not become background source rows.

1. Resolve the current trigger owner, enabled state, model authority and
   owner usage limit through existing mechanisms.
2. Resolve `dot_id`. For a Dot run, require current owner matching and active
   state; construct `Actor::personal_dot` with the owner's actual roles.
   `tools_enabled = false` is an explicit empty-tool ceiling, not a substitute
   role list that might still acquire default group grants.
3. Select a fresh or reusable user-owned session. Validate its binding before
   adding any prompt/assistant turn. Bind a fresh Dot session transactionally.
   An existing bound session cannot be reassigned to a different Dot.
4. Persist the exact run/assistant-turn correlation and execution snapshots
   before any model request. Persistence failure stops the run; existing
   best-effort history writes must not produce an uncorrelated continuation.
5. Load the same personal Dot persona used by interactive chat. Reuse current
   user profile and scoped Dot memories, current selected tools and rules,
   granted MCP connections and the existing durable approval gate.
6. Drive through headless execution. Record `ok`, `waiting` or `error` from
   the exact assistant turn. The existing Inbox announces a suspension.
7. On chat/Inbox approval or timeout, resolve the correlated background turn
   before dispatch. Restore its original usage source and history cap; use
   `headless::drive_resumed` with current owner/Dot context. Retain the original
   approved call and arguments. Recheck owner, Dot active state, user grants,
   current Dot selection/rules and the current trigger's tool ceiling.
   Effective tool enablement is the run snapshot AND current trigger setting.
8. Reconcile the exact run after every terminal continuation or cancellation.
   `Completed` maps to `ok`; `Suspended` stays `waiting`; `Errored` and
   `Cancelled` map to `error` with the existing turn error/cancellation result.
   Update trigger `last_*` summary only when this run is still its newest run.
   Completing an older pending run must not replace a newer fire's summary.
   `InProgress` leaves the run pending, rather than record a premature error.

Disabling a trigger stops new fires; it does not revoke an already waiting
approval. Deactivating or deleting the Dot blocks new fires and continuations.
Editing a trigger's Dot affects future runs. A waiting turn remains bound to
its original Dot; a changed trigger binding cannot turn it into another Dot
or ordinary user execution. Refuse continuation when its current trigger
binding differs from the original run.

Reject trigger deletion with `409` while an exact correlated assistant turn
is generating or suspended. Disabling remains available. A stopped,
completed or errored turn permits deletion; cancellation must reconcile
history so `waiting` cannot remain sticky. The check and deletion are atomic.
An uncorrelated startup run with a null status also blocks deletion until it
is closed by execution failure or the existing interrupted-run startup sweep.

Separate fresh sessions may execute independently. Reuse must prevent two
nonterminal assistant turns from writing concurrently into the same session;
perform the busy check and opening atomically or use the existing session
coordination with equivalent guarantees. This is session serialization, not
a new limit on the number of Dots or background runs. Background continuations
retain headless behavior and do not acquire a new interactive-chat concurrency
cap.

The owner-agnostic session DB provides `create_turn_pair_if_idle` for a fresh
message/assistant pair and `create_assistant_turn_if_idle` for a queued message
or regeneration. Both reserve SQLite's write lock before checking for an
in-progress or suspended assistant and assigning sequence numbers. The pair,
timestamps and session update commit together. Busy returns no inserted turn;
storage failure rolls back the entire insertion. Interactive and headless
callers use this same admission boundary.

Synchronous webhook fires and authenticated reruns that suspend return an
accepted session response with `status: "waiting"`, rather than report a
model failure. Webhook reruns remain fresh conversations and retain the
hook's current Dot binding. Payload retention, secret rotation and untrusted
input handling follow the existing webhook behavior.

## Metering and activity

Initial and resumed calls spend the user's existing budget and retain
`principal_kind = user`, `user_id = owner`, and their original `Scheduled` or
`Webhook` source. Dot names do not change the billing principal. Side calls
continue to use the existing metered path.

Dot tool activity keeps Dot attribution and session/turn correlation through
the existing audit mechanism. This is separate from per-Dot usage aggregation:
`UsageRecord.agent_id` and `chain` exist for agent runs, but the personal Dot
actor currently has no agent run chain. This package must not claim those
fields are populated for personal Dots. Transparent per-Dot usage remains
subsequent work using the same usage records, without a second billing store.

## Adopted UI walkthrough

Use the existing schedule/webhook create and edit forms. Add a “Run as”
selection beside the model/tool settings: ordinary personal assistant or one
of the user's owned active Dots, labelled with actual name/display data.
An existing unavailable/inactive binding remains visible as an unavailable
choice until the owner changes it; it is not relabelled as ordinary execution.

The linked-chat picker requests sessions for the selected binding. Changing
“Run as” clears its existing choice. Existing model selection, cron builder,
timezone, tools toggle, reuse-round cap, synchronous choice and secret reveal
remain their shared mechanisms. A selected Dot's tool access/approval rules
are configured through its existing access disclosure, with a link there.

List rows and run-history pages show the real Dot association and treat
`waiting` as awaiting approval, with the existing Inbox/chat continuation
links. Cancellation/error is distinct from waiting. A rerun that pauses opens
its accepted conversation/approval path. Add convenience links from a Dot card
to the existing forms with its ID preselected; do not create a second trigger
editor under `/dots`.

## Disjoint implementation packages

| Owner | Files and responsibilities |
|---|---|
| Coordinator/schema | Amend `0081`, schema fixture/checksum, owner-agnostic session turn-pair helper, shared documentation/index/API reference and final integration |
| DB/API agent | `aiplane-runtime/src/server/scheduled/mod.rs` and `server/webhooks.rs` storage, Dot-session tombstone and binding queries, `json_workspace.rs`, chat-session filter, webhook trigger/rerun adapter, OpenAPI contracts and route/DB tests |
| Runtime agent | Shared Dot persona/context loader, headless and scheduler worker, exact run lookup/reconciliation, background resume/cancellation integration and runtime tests |
| UI agent | Existing schedule/webhook/Dot pages and components, API clients, six locale catalogs/generated artifacts, TypeScript/E2E coverage |

The scheduled/webhook storage files are shared with the scheduler runtime.
Assign accessor changes to the DB/API agent and worker changes to the runtime
agent; agree accessor signatures before parallel edits. Chat resume and
cancellation API files belong to the runtime package for this change;
chat-session list filtering belongs to DB/API. Schema/catalog/generated files
have a single writer. All build commands use the coordinator's build lock,
`CARGO_BUILD_JOBS=4`, and `mise`.

`aiplane-tools/src/schedule.rs` is another real `NewAction` caller. Its adapter
and fixtures belong to the DB/API package. The adopted
Dot behavior is to inherit `ToolContext.personal_dot.dot_id` for a
schedule created by a Dot, so a selected scheduling tool cannot create an
ordinary-user job outside the Dot's access policy. No webhook-creation tool was
found in the tool crate.

## Acceptance tests and remaining implementation gaps

- Owner isolation and active checks on create, update, linked chat, webhook
  fire/rerun and continuation; deletion cannot erase an execution binding.
- Fresh and reused Dot runs get the same scoped memory/tools/approvals as
  interactive Dot turns. Empty selection and tools-off offer nothing.
- Ordinary-to-Dot, Dot-to-ordinary and Dot-to-other-Dot rebinding reset reuse;
  mismatched sessions and shared chats cannot be linked.
- Exact correlated run survives waiting/resume; approved once executes the
  original arguments only, denial has no tool effect, revocation prevents it.
- Source and owner metering persist across resume; history cap persists;
  terminal/cancelled outcomes update the correct history and newest summary.
- Trigger deletion is refused for nonterminal correlated turns and succeeds
  after stop; interrupted startup sweep retains existing behavior.
- Fresh parallel runs remain independent; reuse cannot overlap nonterminal
  turns; no Dot concurrency cap is added.
- Browser checks cover both existing forms, filtered chat selection, Dot
  association, waiting state, Inbox approval, rerun and responsive layout.
- Mock upstream/MCP calls and in-memory SQLite keep tests offline and
  state-based. Tool side-effect absence is checked through observable state.

The loader/reconciler signatures and unavailable-state enum above are adopted.
The coordinator owns this owner-agnostic session DB helper:

```rust
create_turn_pair_if_idle(
    pool: &Pool,
    session_id: &str,
    user_turn_id: &str,
    assistant_turn_id: &str,
    prompt: &str,
    model: &str,
    reject_pending: bool,
) -> Result<Option<(Turn, Turn)>, DbError>
```

It atomically checks for `InProgress`/`Suspended` assistant turns, creates the
user/assistant pair with consecutive sequence numbers, and touches the
session. Background admission sets `reject_pending` to reject queued user
messages too. `None` means busy. The helper treats conversation ownership as
opaque; the runtime validates user and Dot bindings.

Startup reconciliation updates waiting history whose surviving turn became
completed, cancelled or errored. A genuinely suspended turn retains its
approval. History for a deleted conversation retains its deleted-chat marker;
continuation requires its actual session and turn.

Automatic memory
consolidation and persistent Dot computer/workspace lifecycle are separate
remaining packages.
