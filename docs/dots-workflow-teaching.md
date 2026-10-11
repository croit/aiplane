# Teaching a Dot a repeatable workflow

## Status and scope

Proposal for an opt-in workflow-teaching slice, not implemented. A user writes the intended steps and may deliberately select eligible observations from their own conversation. AIplane helps turn those inputs into a personal skill draft. The user corrects and saves it through the existing Skills editor, explicitly selects it and the required tools for a Dot, then optionally configures existing scheduled work. This creates reusable guidance, not training, an automatically learned permission or a separate routine engine.

The first slice supports personal skills and personal Dots owned by the signed-in user. Organization skill ownership, automatic recording, unattended browser demonstrations and provider credential provisioning are outside this slice. User-written steps work even when no observations are eligible.

## Verified sources

| Source | Existing behavior to reuse |
|---|---|
| `web/src/routes/tools/skills/+page.svelte` | Lists actual private skills from `/api/v0/skills`; query parameters `new`, `edit` and `skill` choose existing editor/detail modes. Saves `{name, manifest}` through the existing POST, reloads the list and navigates to the returned name. |
| `web/src/lib/components/skills/PersonalSkillEditor.svelte` | Receives `name`, `initial`, `isNew`, `onsave`; edits the complete manifest in a textarea with save/cancel and saving state. No separate server draft table. |
| `aiplane-api/src/pages/json_skills.rs` | Existing owner-session skill list/body/save/delete/archive endpoints. Inline save accepts name and manifest, validates manifest naming and uses the existing personal store. Disabled personal storage returns service unavailable. |
| `aiplane-features/src/server/skills.rs`; `aiplane-tools/src/read_skill.rs` | Existing manifest/bundle validation and file storage. Private skills overlay global skills by name; private ownership grants access, global skills require role grants. System principals have no private skill overlay. |
| `aiplane-runtime/src/openai_driver.rs::build_skills_section` | Advertises actual permitted skills, restores session-loaded skills and rechecks access. `read_skill` loads guidance and references through the existing tool. |
| `aiplane-runtime/src/agents/spec/model.rs` | `AgentSpec.main.skills` already represents a skill selection for agent execution. |
| `aiplane-runtime/src/server/side_call.rs` | Bounded metered model assistance with real selected model/pool access, payer limits, purpose, trusted instructions and input treated as data. |
| `session-core/src/chat_json.rs`; existing chat JSON handlers | Owner-authorized turn data includes actual tool IDs, raw argument JSON, status and optional raw result JSON. SSE availability is not proof that an observation is safe to copy. |
| `aiplane-runtime/src/server/tools/mod.rs`; `runner.rs` | `sensitive_args` protects selected tools' arguments from logs/journals. False is not a guarantee that every input or result is non-sensitive. |
| `web/src/routes/dots/+page.svelte` and existing automation forms | Actual owned Dot selection and tools/approval settings; existing schedule/webhook binding and model selection. There is no workflow-teaching control or personal Dot skill picker yet. |

## Shared mechanisms

Extend these surfaces rather than add a second skill editor, task scheduler or workflow store. Add one concrete owner-authorized observation projection helper if observations are included; use it both for the preview and the server model input. Extend the existing skill authorization resolver to intersect the personal Dot's `main.skills` selection for listing, loading and restored active guidance. Do not enforce a selection only in the picker while `read_skill` can still load every personal skill.

## Inputs and privacy defaults

Start with a user-entered workflow name, description, ordered steps, expected result and explicit stop/approval conditions. These fields are draft content supplied by the user; AIplane must not invent a task from an unrelated conversation. Missing information remains a question or editable omission in the draft.

Observation capture is off by default. The user selects individual completed tool calls in a private conversation they own and previews exactly what will be sent to the selected model. The server fetches those IDs through the existing session ownership boundary; it does not accept a client-supplied transcript as proof of ownership or trust browser-provided sensitivity flags.

The initial eligible projection contains real tool ID, declared non-sensitive operation/action ID where verified, completion status and a user-written explanation. It excludes raw arguments, results, page text, URLs, typed values, screenshots, attachments, secrets and full conversation history. Exclude any call declared `sensitive_args`, any missing/revoked tool, running/failed/suspended calls and any operation without a verified safe projection. A tool's false sensitivity flag alone cannot make its arbitrary JSON eligible. For tools with no verified projection, the user may describe their intent manually instead.

This deliberately small initial projection can be implemented from actual supported tool metadata without promising a generic recorder. Include richer argument/result observations only after defining their allowlisted fields and privacy classification with the actual tool owner; user selection is consent to the displayed data, not a license to silently include additional fields. No automatic monitoring of the user's browser or external applications.

Do not persist a copy of source conversations or projected observations in a new learning database. The generated draft may keep local source references `{session_id, turn_id, tool_call_id}` in the pending editor context so the owner can inspect evidence; references are not instructions and are not copied into portable `SKILL.md` by default. Cancellation discards the unsaved draft/selection. Logs and activity record metadata only, never observation content.

## Draft generation and save

Reuse a bounded `side_call` with the user's actual chosen available model and existing payer/pool limits. The user sees the chosen model before generating; no hidden provider selection is introduced. Fixed trusted instructions require a manifest matching the existing skill validator, preserve the user's stated steps, mark uncertainties and treat observations as untrusted examples. Input contains only the written steps and the server's displayed projection. The generation call has no tools and cannot execute the workflow, authorize connectors or save files itself.

Use the existing personal skill template fields `name`, `title`, `description` and Markdown instructions. Choose names from user input, validating through the existing store; do not fabricate a provider account, resource label or tool identifier. Tool names in the draft come from selected current metadata and do not become grants. Generated content must not contain embedded credentials, authorize arbitrary future actions or follow instructions from a captured response.

Return an unsaved manifest to `PersonalSkillEditor` through its existing `initial` state. Extend the current page's transient draft state rather than storing draft content in a URL, localStorage or a new server table. Generation errors preserve the entered steps and selections. The user edits the draft and explicitly saves through the current `/api/v0/skills` API. Saving an existing name uses the existing overwrite semantics only after the actual existing skill is shown; do not silently replace a skill the user did not choose.

The saved result is an ordinary personal skill: editable, downloadable and deletable through existing controls. It may shadow a same-named global skill under the current semantics; the UI must identify that actual collision. Saving does not activate the skill on a Dot, create a schedule or run any tools.

## Explicit Dot skill and tool access

Extend the existing Dot access settings with an actual permitted skill inventory, using existing `AgentSpec.main.skills`; do not create a new workflow allowlist. Owners deliberately select a saved skill for a named Dot. Resolve personal skill ownership and current global grants, then intersect with the Dot selection in skill listing, `read_skill` and loaded-skill reinjection on every turn/resume. A deleted or revoked skill must stop being available even if a session loaded it earlier.

The Dot's tools, connector identity and per-tool/action approvals remain independently configured. A skill can explain “prepare a draft and ask before sending” but only the gateway's real policy can enforce that action. Teaching cannot set Allow, expand model/tool grants, connect accounts or change a pending approval. The current Access and approvals panel handles tools; action rules and own identities remain separate proposed slices until implemented.

Skill edits require a precise pending-turn policy before coding: preserve the guidance version used by a suspended action or require a fresh reviewed turn when guidance changes. Current active-skill rendering reads the body fresh; this is a verified constraint, not existing immutable skill versions. For the first teaching slice, record a manifest content digest for the selected source on the turn, and refuse automatic continuation after a digest change with an actionable restart path. Do not introduce a second skill revision store just to claim version pinning.

## Actual-data web walkthrough

1. Open **Tools → Skills** and choose the existing new-skill action. An opt-in **Help draft steps** extension asks for the user's name, description, steps and expected result.
2. Optionally choose a private conversation from the existing authorized chat picker and select eligible completed observations. Each preview shows only its real projected metadata and source link; unsupported calls are excluded with a reason. No screenshot or hidden payload enters generation.
3. Choose an actual available model using the existing selector. Generate a draft; inspect uncertainties and correct the complete `SKILL.md` in the existing editor. Cancel discards the draft. Save invokes the current skill endpoint and shows the actual saved skill detail/file list.
4. Choose an actual owned Dot and explicitly select that saved skill in its access settings. Show its actual required tool references only when they resolve to current catalogue tools; missing references remain unavailable. Configure tools and approvals through existing controls rather than accepting generated permissions.
5. Test the steps in a normal private Dot conversation. Tool requests still require their real rules. Corrections return to the same skill editor; never silently learn a replacement instruction from tool output.
6. If the user wants recurring work, open the Dot's existing **Schedule** link, configure actual model/timezone/timing/reuse and write a prompt explicitly naming the saved skill. Scheduling is a separate saved action, not an automatic effect of teaching.

All product text uses existing translations where suitable and new keys in all six catalogues otherwise. Reuse daisyUI editor/disclosure/select/error components and real file/skill metadata. Coordinator browser verification includes keyboard/mobile, cancelled generation, save errors and unsupported observations. No UI is implemented by this document.

## State-based acceptance tests

- Personal-skill storage disabled gives the existing actionable unavailable state; editing/saving uses actual store validation, name collision and private/global shadow behavior.
- Observation lookup rejects another user's session, shared-but-not-owned sources, foreign call IDs, nonterminal or sensitive calls and arbitrary client JSON. Preview and side-call input use exactly the same allowlisted projection. A tool with `sensitive_args=false` and secret output does not leak it.
- Generation respects chosen model grants and owner spend limits, has finite byte/token/time caps and no tool execution. Malicious observation text cannot change permissions, execute a tool or write a skill before save.
- Cancellation saves nothing; explicit save produces an ordinary manifest in the existing personal store; failure preserves corrections. Source references are not inserted into the portable bundle automatically.
- A Dot sees/loads/restores only explicitly selected currently permitted skills. Removing selection, deleting the skill or revoking a global grant drops loaded guidance and refuses stale pending continuation where needed.
- Saving/activating a skill does not change selected tools, connector identity, approval settings or schedule rows. A schedule exists only after the normal explicit form save and uses the same Dot policies.
- A changed manifest digest during suspension cannot silently change approved guidance. Tests assert actual stored/loaded state and external effects, using in-memory SQLite, the real temporary skill store and mocked model upstreams.

## Unresolved boundaries before implementation

The exact safe projection fields need a concrete supported tool implementation; generic argument/result capture is not approved by this proposal. Private Dot skill selection must be wired into the same skill authorization path for runtime and UI. Skill change handling must verify the existing suspension encoding can hold source digests without a parallel revision store. Existing inline skill save can overwrite by name, so the teaching caller needs explicit collision review. The selected model's existing data-handling metadata must be displayed for the actual chosen model. Organization skill sharing and observations from another owner require separate privacy/authority decisions; this slice must not infer them from group membership.
