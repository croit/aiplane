# Personal Dot memory curation

## Status and scope

Implementation proposal prepared under the user's delegated product decisions. Automatic memory curation, provenance, reviewed learning observations and optimized Dot memory are **not implemented**. Existing personal Dot memory, its tool rules and ordinary scheduled Dot runs are implemented. This document proposes the next bounded vertical slice; it does not describe an available setting.

The requirement is durable learning across runs: useful individual lessons belong to their Dot, intentional personal preferences belong to the user, and a regular background job prepares a smaller useful memory context. This is inference over permitted stored evidence, not model training. Peer communication and organization membership must not promote information into personal memory or authorize reading another Dot's notes.

The design recommendations below are chosen defaults for parent review, not facts inferred from OpenDots or the current implementation.

## Verified implementation

| Source | Existing behavior | Constraint for curation |
|---|---|---|
| [`aiplane-tools/src/memory.rs`](../crates/aiplane-tools/src/memory.rs) | `remember`, `recall`, `update_memory`, `forget`; `MemoryKind` is preference/project/fact; tool content is bounded to 2,000 bytes and recall to 200 records. Personal Dot writes target only its own Dot store. `recall` combines that Dot's and the owner's notes with explicit scopes. | Reuse the four actual tools. A Dot cannot write user-wide memory by choosing `scope: user`. |
| [`aiplane-agents/src/db/dot_memories.rs`](../crates/aiplane-agents/src/db/dot_memories.rs) | SQL checks the personal owner and Dot; list/get/insert/update/delete; exact duplicate insert reclassifies kind; rows expose id/kind/content/created_at. | No provenance, manual-versus-learned flag, input revision or reviewed-evidence field exists. An exact duplicate is not inherently a no-op. |
| [`aiplane-core/src/server/db/user_memories.rs`](../crates/aiplane-core/src/server/db/user_memories.rs) | Per-user CRUD, kinds, recency/substring retrieval and counts. SQL stores updated_at, which `Memory` does not expose. | User memory is the existing profile store; do not create another preference engine. Current update APIs do not provide compare-and-swap. |
| [`persona/personal_dot.rs::context_for`](../crates/aiplane-runtime/src/persona/personal_dot.rs) | Owner/session binding, current active state and spec; up to 40 Dot notes under the existing memory gate, bounded to 16,000 serialized bytes; saved notes explicitly untrusted. | Derived context must preserve this gate and data boundary. An unavailable Dot must not curate. |
| [`openai_driver.rs::preferences_in_context`](../crates/aiplane-runtime/src/openai_driver.rs) | `recall` must be granted and enabled at account level. Standing user preferences consider 50 notes with a 3,000-character budget; facts/projects remain available through recall. | Account memory-off also means no injection. Dot-selected tools are an additional ceiling; an account grant alone is insufficient for the job. |
| [`persona/live.rs::LivePersonalDotTool`](../crates/aiplane-runtime/src/persona/live.rs) | Rebuilds current owner, active Dot, typed spec, grants and settings before tool execution; intersects initial access and current ceilings; applies existing permissions. | Use the live wrapper for approved curation writes, including after suspension. |
| [`agents/approval.rs`](../crates/aiplane-runtime/src/agents/approval.rs) | Typed tool/action rules, AskFirst and durable original-call approval behavior. | Memory tools are separate named operations; they have no `action_catalogue` today. Do not invent a generic `memory.write` action. |
| [`server/side_call.rs`](../crates/aiplane-runtime/src/server/side_call.rs) | `ask_json`, explicit schema, capped answer, timeout, model routing, payer limits and usage; returns a `SideExchange`. Schema requests still need typed local validation. | Planner uses this mechanism, not another HTTP/model client. It has no automatic authorization to mutate memory. |
| [`server/compaction.rs`](../crates/aiplane-runtime/src/server/compaction.rs) | Post-turn summary side call with the actual conversation model, access and payer; source turns retained; `chat_compactions` represents a session prefix. | Reuse bounded planning/side-call principles. Chat-prefix compaction is neither a personal profile nor a durable lesson store. |
| [`server/scheduled/worker.rs`](../crates/aiplane-runtime/src/server/scheduled/worker.rs), [`scheduled/mod.rs`](../crates/aiplane-runtime/src/server/scheduled/mod.rs) | Existing polling, cron/timezone, enabled state, persisted history, startup reconciliation and headless owned Dot sessions. `claim_due_in` atomically checks due time, enabled state and configuration generation in a WriteTx before advancing the slot and admitting its run. | Extend this scheduler's task execution and reuse its conditional claim; no second timer or queue. |
| [`server/headless.rs`](../crates/aiplane-runtime/src/server/headless.rs), [`server/background.rs`](../crates/aiplane-runtime/src/server/background.rs) | Owned private run opening, shared worker registry, cancellation, suspension and run/turn correlation. | A curation application turn must be a real correlated run, not an invisible direct database write. |
| [`server/model_choices.rs`](../crates/aiplane-runtime/src/server/model_choices.rs) | Shared actual model/alias/automatic-route choices constrained by PoolAccess. | Reuse it and the existing model selector. `json_workspace::chat_model_options` currently lists upstream models; do not copy that helper as a complete alias/route/owner authorization mechanism. |
| [`agent_audit/redaction.rs`](../crates/aiplane-agents/src/db/agent_audit/redaction.rs), [`session-core/db/tool_calls.rs`](../crates/session-core/src/db/tool_calls.rs) | Audit redaction knows secure resumed values and sensitive tool arguments. Tool rows have running/completed/errored status and capped output. | These are not universal non-sensitive evidence flags. Truncation, a successful result or an assistant answer does not prove safe content. |
| [`DotMemoryPanel.svelte`](../web/src/lib/components/dots/DotMemoryPanel.svelte), [`json_dots.rs`](../crates/aiplane-api/src/pages/json_dots.rs), [`json_workspace.rs`](../crates/aiplane-api/src/pages/json_workspace.rs) | Existing owner memory management, actual entries and scoped CRUD; separate user memory UI/API. | Extend these surfaces; no second note editor. Current manual CRUD is not a model tool call and must stay distinguishable from autonomous writes. |

Storage originates in migrations 0008/0009 and 0081. The parent owns amendments to the current unreleased 0082 change set and its fixtures/checksum. This document changes no migrations.

## Shared mechanisms

1. Typed `AgentSpec`, current personal Dot identity/session binding, `GrantedToolSource`, live personal tool wrapper and existing action/tool approval rules.
2. Existing scheduled action cron/timezone/enable/model fields, scheduler worker, headless session opening, worker registry, cancellation and scheduled run reconciliation.
3. `side_call::ask_json`, `Payer::person`, `UsageSource::Scheduled`, `PoolAccess`, `model_choices::offered` and normal model routing. Personal curation is charged to its owner; current usage does not establish a separate per-Dot spending account.
4. Existing note CRUD/types and bounded context readers. Canonical notes remain the source; a derived summary must never overwrite them.
5. `chat_turn_suspensions`, AskFirst, Inbox, current call revalidation and original argument pinning. `runner::execute_tool_calls_guarded` alone does **not** persist/resume a complete turn: the OpenAiDriver's classification, persistence and pause helpers also matter.
6. Existing audit redaction and outbound/input handling. Before a new planner caller sends evidence, expose/reuse the relevant normal input-policy admission from the driver; `side_call` alone is not evidence that the interactive chat's complete content-policy path ran.

Necessary shared prerequisites, with real curation callers in the same slice:

- Versioned note/evidence snapshots and conditional publication/application. Notes currently lack usable revision/provenance metadata.
- Reuse the conditional claim of the existing due scheduled slot; add a per-Dot curation lease covering planner/publication/application. Use database ownership, not a second scheduler. The worker registry governs a conversation turn, not all different curation sessions for the same Dot.
- If applying planner-produced calls without another model turn, extract the existing durable tool-call classification/execution/pause path into one reusable turn mechanism. Both OpenAiDriver and curation must use it. Do not synthesize approval rows or invoke database CRUD around the live tool wrapper. The source scout has verified the pieces, not an already complete standalone public API.

These prerequisites land with state-based tests before feature consumers. Keep Dot-aware persistence in `aiplane-agents`, orchestration/model calls in `aiplane-runtime`, HTTP admission in `aiplane-api`, and existing tool execution in `aiplane-tools`. `session-core` remains owner-agnostic; core's migration placement does not authorize putting Dot orchestration in core.

## Recommended first slice

### 1. Opt-in sources and periodic planning

Curation starts disabled. The owner enables it for one active personal Dot and explicitly selects existing Dot note IDs eligible for optimization. Reviewed text observations from that Dot's own completed, private, non-delegated runs are an additional source. An observation is a selected, editable excerpt that the owner marks as safe to retain; no current observation flag exists. Do not scan all transcripts, attachments, MCP payloads, browser state, credentials, secure-input responses or raw error strings. Neither peer messages nor child transcripts become evidence by being in the same chain.

Existing notes may themselves contain secrets. Selecting a note is not proof of automated secret detection: the UI must show the actual content and request deliberate selection of non-sensitive learning material. Exclude known secure/sensitive origins before presenting an excerpt; after review, retain only the reviewed excerpt and its source reference, not a hidden copy of the full original payload. Automated detection may help later, but no universal detector exists to promise today.

Recommended initial schedule: once daily, using the owner's existing timezone when present and an explicitly displayed UTC fallback otherwise. Reuse the cron editor to change it. Require an explicit model choice from current owner-permitted choices; no special curation model, hardcoded identifier or all-pools default. Skip absent new input or unchanged source versions without a model call. Do not queue missed days as a backlog. At most one planner attempt per due slot; a failed attempt is visible and can be retried explicitly.

Proposed finite limits for parent confirmation: 40 selected notes, 20 reviewed observations, 24,000 UTF-8 input bytes including metadata, 2,000 output tokens, 60-second planner timeout, and five proposed note operations. Report excluded/truncated source IDs and do not split a note or excerpt to invent complete evidence. These are new design bounds, not current worker settings. Use the actual model context window to reduce the batch further when necessary; reject a batch that cannot fit.

### 2. An optimized view, not replacement notes

One metered `side_call::ask_json` receives immutable, owner-scoped source records with IDs/revisions and explicit instructions to treat all evidence as untrusted data. Its typed result contains a bounded Dot-context summary and zero or more lesson proposals with supporting source IDs. It may not emit tools, settings, identities, grants, arbitrary SQL, new URLs to fetch, or changes to user preferences.

Validate every referenced ID against the exact input manifest, lengths/kinds/counts against local limits, and current authorization against the snapshot. Reject malformed output rather than partially publishing it. Require output with no operational authority: a statement in learned text cannot become a system instruction, tool rule, skill, credential, schedule or delegation permission.

Publish the derived Dot summary only if the snapshot and lease remain current. Canonical notes and observations remain inspectable and unchanged. The persona can use the current summary as **untrusted Dot data** in its existing byte budget, with actual source-note IDs and revisions; it must preserve reviewed/manual notes not represented by that summary within the same overall budget, or explicitly report their exclusion. No summary is allowed to hide or silently replace a manually entered constraint. Stale, invalidated or unavailable summary falls back to the existing bounded note reader.

User preferences continue through the existing user-profile reader. Do not blend them into the Dot summary or materialize copies in each Dot. A future optimized user preference view requires its own owner-selected sources and the same versioned derivation mechanism, not promotion of the first Dot summary that sounds general.

### 3. Reviewed lesson application

A proposed new lesson is not a saved memory. Show its exact kind/content/sources and let the owner reject or correct it. Accepted candidate text becomes immutable application arguments for the existing `remember` tool in a correlated private Dot run. Default to Ask before each call for this workflow; the owner may use the actual selected tool rule to permit unattended additive writes after explicitly enabling that behavior. Selection/grants, active state, account memory switch and the narrower curation-only source are still mandatory. Curation offers no MCP, browser, file, delegation or unrelated tools.

Never update or forget a manual note automatically. Updates/deletions, including consolidation of generated lessons, need explicit selection of the affected records and review of actual old/new content. Execute through the existing `update_memory`/`forget` rules and original-argument suspension store; unavailable tools remain unavailable. A global Allow setting is not permission for the curator to mutate notes outside its selected provenance scope. The initial application scope can conservatively remain additive only; do not claim a completed deletion/consolidation engine in that stage.

Per-tool rules are the action rules here: remember, update_memory and forget are distinct actual actions. Do not infer safety from their names or manufacture undocumented action enums. Approve once authorizes the original call only; note revisions, curation manifest, current access and Dot active state are rechecked before mutation. A changed source/target yields a visible stale-plan failure and regeneration, never a silently rebased approval.

User-wide preference promotion is a separate deliberate owner action in the existing user Memory page: review the actual candidate text and save it there through its authenticated user-owned API. It is not a Dot tool invocation. Curation never changes the Dot tool scope to user, never chooses a peer, and never emits organization-owned data into a personal profile. Group/organization knowledge is excluded from this personal slice.

## Proposed data model amendment

Amend only the parent's unreleased migration, schema fixture and checksum after contract approval. Minimum persisted facts, using the existing SQLite database:

- Existing note tables gain a monotonic revision used by snapshot validation; every real insert/reclassification/update increments it. A deleted ID is never replaced under the same ID. Existing manual/tool origins are unknown unless actually recorded: legacy rows start `unclassified`, not guessed `manual` or `learned`.
- Dot memory provenance links a real note ID to source kind, originating turn/call where known, and curation run where applicable. Ordinary manual edits mark the changed revision owner-edited; curation cannot retain automatic rewrite authority after such an edit.
- An owned Dot curation settings record links to **one existing scheduled action** and contains settings generation, explicitly selected note IDs and opt-in state. Extend ScheduledAction with a typed execution kind (ordinary prompt versus Dot memory curation) and validated owned binding; reuse its cron/timezone/model/enable fields. No invisible parallel schedule record or timer.
- Owned reviewed observation records contain Dot ID, source session/turn reference, review revision, reviewed text and timestamps. Source deletion/rebinding invalidates future planning; retain no raw secret-bearing payload. Reference deletion and owner-requested erasure invalidate derived output and remove the retained excerpt according to the same policy.
- One derived artifact per Dot stores summary, input manifest, revision, planner run/turn reference and generation time. Candidate operations and their evidence/target revisions are a typed plan associated with that existing scheduled run; they are not another approval queue. Discarded/expired plans may be deleted independently without deleting canonical notes.
- A leased curation attempt carries an opaque generation and expiry; exact run/turn references join existing history. Conditional slot claiming and lease renewal must not revive cancelled work. Store a bounded retryable interrupted outcome on startup.

Snapshot revision checks and application mutations must occur in one `WriteTx` transaction. A pre-read followed by unconditional note update leaves an approval/edit race. Scope-conditioned compare-and-swap belongs in the existing note accessors and is used by the actual tool caller, not an alternate curator CRUD implementation. Atomicity of same-Dot plan publication does not imply atomic application of several separate approved tool calls: record each actual outcome and regenerate remaining operations after conflicting edits.

Do not store full model exchanges as a second dataset. Use existing exchange/audit mechanisms where appropriate and apply existing redaction; the owned run history links to the actual execution. SideExchange may contain reviewed source text, so access and retention must be no broader than those sources. Audit metadata can carry IDs, versions and counts without duplicating note contents.

## Web walkthrough using actual data

The following is a proposed acceptance walkthrough; Harald and its notes are created through the real fixture/API, not shipped defaults.

1. Create the personal Dot **Harald**. Its existing Memory disclosure shows actual stored notes, kinds and content. Extend that panel with opt-in curation status, source selection and the latest derived summary/real timestamp. Reuse existing editors, actual Dot name and API records.
2. The owner selects an existing project note and a safe reviewed excerpt from Harald's own completed run. Selecting a transcript reference shows the actual source and the editable excerpt; unsupported/removed sources are unavailable, never given fabricated text.
3. Configure a daily schedule in the existing schedule form with Harald as its actual Run as binding and a permitted model through the current selector. Source settings remain a typed curation field; ordinary prompt automations retain their behavior. Show actual timezone and next run returned by the server.
4. Run once through the same scheduler/history path. The result shows selected input count, excluded IDs, summary and exact proposed note text. No notes have been overwritten. Open the actual source conversation using existing private-history links.
5. Accept one lesson with remember set to Ask. The existing Inbox shows the original content/kind call. Approve once; the existing Dot Memory panel then shows the actual saved row with provenance. Rejecting leaves canonical memory unchanged.
6. Edit a source note while approval is pending. Approval must fail visibly as stale without overwriting that edit. Reload/regenerate shows the actual updated content.
7. A proposed broad preference is offered as text only. The owner explicitly opens their existing user Memory editor, reviews it and saves it. Clara, another Dot, receives the preference only through the existing permitted user context; it never receives Harald's private lessons.
8. Disable curation or deactivate Harald: pending application cannot continue. Existing notes remain. Reactivation does not silently restore discarded approvals. Deleting the Dot cascades owned artifacts, selections and excerpts and disables/removes its linked curation schedule in the same lifecycle contract. Personal erasure removes derived copies without touching unrelated user notes.

Desktop/mobile, keyboard disclosure/checkbox/editor operation, loading/error/reload states and source conflicts need actual browser verification. New text belongs in all six Fluent catalogs with formal German address. No UI, key or screenshot is created by this proposal.

## Acceptance tests and implementation order

Use real SQLite, the existing crypto/redaction functions, actual memory tools, worker registry and wiremock model upstreams. Inputs and polling are finite; do not use interaction mocks for database ownership or approvals.

1. **Shared prerequisites:** schema round-trip/constraints; revision increments on duplicate reclassification, manual/tool edits and conditional failures; no foreign owner/Dot reads; exact manifest comparison; atomic claim with two scheduler tasks and one due slot; concurrent planners publish once; expiry/startup/cancellation cannot revive a stale lease.
2. **Pure bounded planning:** zero/unchanged input makes no call; selection and byte limits; unsupported kinds/IDs/duplicates; untrusted instructions retained only as data; all output references must match evidence; partial/malformed/oversized model output never publishes.
3. **Metered model path:** actual allowed model/alias/automatic route; forbidden pool rejected; owner limits reject before upstream execution; successful/failed call usage recorded once; timeout/non-JSON/capped response; ordinary input-policy requirements preserved. No implicit external model selection.
4. **Context:** recall disabled or revoked prevents both planning and injection; inactive/deleted Dot blocked; selected recall missing blocked; owner profile remains separate; bounded valid summary plus uncovered manual notes; revision mismatch falls back to original reader; no peer/organization/child-session data.
5. **Application:** actual remember Ask produces a real suspended turn/Inbox; no memory before approval; original args after resume; owner/tool revocation/deactivation and source/target edits refuse before mutation; Allow cannot bypass source scope; unrelated/MCP tool calls rejected; manual notes never automatically updated/forgotten; retries do not duplicate already applied operations.
6. **Routes/lifecycle:** owned versus foreign IDs, management permission for settings, read policy unchanged, deletion cascades and source erasure invalidation, no secret-bearing errors; ordinary scheduled prompts and other Dots unaffected.
7. **Browser:** actual Harald sources/model/schedule, source review, one pending approval, saved provenance, source-edit conflict, deactivate/revoke, explicit user-profile save, mobile/keyboard and reload. Parent coordinator verifies actual pages before completion.

First deliver the shared revision/conditional claim/durable application seam; then one owned opt-in daily planner with derived context; then additive lesson review/application through existing approvals. User preference promotion and automatic rewrite of learned notes must remain explicitly incomplete until their real callers and tests are present. No learned-text-to-permission, skill or organization promotion is part of this design.

## Verified gaps and decisions to settle before code

- There is no provenance or universal sensitivity classification on current note/turn records. Opt-in reviewed excerpts and exact selected notes are the proposed minimum; automatic scanning of raw runs is excluded.
- Existing scheduled slots use conditional admission. A curation lease still cannot guarantee exactly-once upstream billing across process death after a response.
- Existing side calls provide routing/metering, not the complete interactive input-policy/approval lifecycle. Reuse/extract those shared seams before a new caller; do not implement a second approval engine.
- The proposed daily cadence and finite limits are chosen recommendations requiring parent contract confirmation. Model selection is deliberately explicit because no dedicated curation default exists.
- Safe optimized coverage cannot be proved by an LLM's claim that every source was preserved. Keep canonical notes and a source manifest, preserve uncovered manual content, and show excluded sources; do not label the output complete knowledge.
- A universal guarantee that arbitrary manually entered text contains no secrets is unavailable. Known secure origins are excluded and selection/review is explicit; no speculative detector or safety claim is included.
- Organization membership, scoped memory and private published execution are implemented. Organization curation remains outside this personal slice; it supplies no shared-memory promotion or employee-profile access.
