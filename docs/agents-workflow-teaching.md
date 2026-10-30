# Teaching an agent a repeatable workflow

## Status and scope

The first workflow-teaching slice is implemented for personal skills. A signed-in user enters a name, goal, ordered steps, stop or approval conditions, and one available chat model. AIplane returns an unsaved `SKILL.md` draft in the existing personal Skill editor. The user reviews, corrects, and explicitly saves it. Draft generation is metered to the user and respects the same spend limit and model access as other side calls. It does not run the workflow, enable a tool, grant a skill to an agent, create a schedule, or persist the source fields before the user saves.

This slice supports personal skills. Conversation observations, organization skill ownership, automatic recording, unattended browser demonstrations, personal agent skill selection and provider credential provisioning are not part of the implemented flow. A saved skill remains ordinary personal skill content and must be selected through an available agent skill access mechanism.

## Verified sources

| Source | Existing behavior to reuse |
|---|---|
| `web/src/routes/tools/skills/+page.svelte` | Lists actual private skills from `/api/v0/skills`; query parameters `new`, `edit` and `skill` choose existing editor/detail modes. Saves `{name, manifest}` through the existing POST, reloads the list and navigates to the returned name. |
| `web/src/lib/components/skills/WorkflowDraftDialog.svelte` | Collects only user-entered workflow fields, uses the existing chat model list and returns the unsaved manifest to the existing editor. |
| `web/src/lib/components/skills/PersonalSkillEditor.svelte` | Receives `name`, `initial`, `isNew`, `onsave`; edits the complete manifest in a textarea with save/cancel and saving state. No separate server draft table. |
| `aiplane-api/src/pages/json_skills.rs` | Existing owner-session skill list/body/save/delete/archive endpoints. Inline save accepts name and manifest, validates manifest naming and uses the existing personal store. Disabled personal storage returns service unavailable. |
| `POST /api/v0/skills/draft` | Checks the caller's offered chat models, meters the side call to the caller, bounds input and output, and returns an unsaved manifest. It does not write to the skill store. |
| `aiplane-features/src/server/skills.rs`; `aiplane-tools/src/read_skill.rs` | Existing manifest/bundle validation and file storage. Private skills overlay global skills by name; private ownership grants access, global skills require role grants. System principals have no private skill overlay. |
| `aiplane-runtime/src/openai_driver.rs::build_skills_section` | Advertises actual permitted skills, restores session-loaded skills and rechecks access. `read_skill` loads guidance and references through the existing tool. |
| `aiplane-runtime/src/agents/spec/model.rs` | `AgentSpec.main.skills` already represents a skill selection for agent execution. |
| `aiplane-runtime/src/server/side_call.rs` | Bounded metered model assistance with real selected model/pool access, payer limits, purpose, trusted instructions and input treated as data. |
| `session-core/src/chat_json.rs`; existing chat JSON handlers | Owner-authorized turn data includes actual tool IDs, raw argument JSON, status and optional raw result JSON. SSE availability is not proof that an observation is safe to copy. |
| `aiplane-runtime/src/server/tools/mod.rs`; `runner.rs` | `sensitive_args` protects selected tools' arguments from logs/journals. False is not a guarantee that every input or result is non-sensitive. |
| `web/src/routes/agents/+page.svelte` and existing automation forms | Actual owned agent selection and tools/approval settings; existing schedule/webhook binding and model selection. There is no workflow-teaching control or personal agent skill picker yet. |

## Shared mechanisms

The dialog is a small extension of the existing personal Skills page. It uses `Modal`, `SearchableSelect`, `/api/v0/models`, `model_choices::offered`, `side_call`, and the existing `PersonalSkillEditor` and `/api/v0/skills` save endpoint. It adds no workflow database, new model catalog, scheduler, or tool execution path.

## Inputs and privacy defaults

The dialog accepts a user-entered workflow name, goal, ordered steps and explicit stop or approval conditions. The API requires 1–20 non-empty steps, each capped at 1,200 UTF-8 bytes; the name is limited to 160 bytes and the goal and stop conditions to 1,200 bytes each. It removes blank steps and rejects a request when every step is blank. Missing information remains an editable omission in the generated draft.

This implementation does not read conversations, tool calls, browser state, or other source material. Only the four text fields entered in the dialog are sent to the user-selected model. The form and model response remain transient in the browser until the user explicitly saves the manifest through the existing Skills API. Usage metering records model and token metadata through the standard usage path, not the workflow text.

## Draft generation and save

Reuse a bounded `side_call` with the user's actual chosen available model and existing payer/pool limits. The user sees the chosen model before generating. The server recomputes available models from the caller's current roles and rejects model IDs absent from that set. Fixed trusted instructions treat the submitted fields as untrusted content and require the output to preserve the stated steps without inventing tools, accounts, facts, or permissions. The generation call has no tools and cannot execute the workflow or save files itself. The JSON response is capped at three fields and 12,000 instruction characters; the server assembles and checks the skill frontmatter.

Use the existing personal skill fields `name`, `title`, `description` and Markdown instructions. The server derives the filesystem-safe skill slug from the entered name and uses the model for the title, description and instructions. The existing personal Skills save endpoint remains the authority for validating and writing the final manifest. The generated content does not grant tools or authorize external actions.

Return an unsaved manifest to `PersonalSkillEditor` through its existing `initial` state. The page holds the generated content in transient component state, not a URL, localStorage or a new server table. Generation errors preserve the entered fields in the dialog. The user edits the draft and explicitly saves through the current `/api/v0/skills` API.

The saved result is an ordinary personal skill: editable, downloadable and deletable through existing controls. It may shadow a same-named global skill under the current semantics. A create request cannot overwrite a same-named personal skill; editing an existing skill is explicitly marked as an overwrite. Saving does not activate the skill on an agent, create a schedule or run any tools.

## Explicit agent skill and tool access

Personal agent skill selection is not part of this implementation. The existing agent resource model already carries skill references, but the personal agent access UI/runtime path must be extended consistently before this flow can promise selection for every agent. Do not add a new workflow allowlist. A future change must enforce selection in listing, `read_skill`, and loaded-skill reinjection on each turn/resume, not only in a picker.

The agent's tools, connector identity and per-tool/action approvals remain independently configured. A skill can explain “prepare a draft and ask before sending” but only the gateway's real policy can enforce that action. Teaching cannot set Allow, expand model/tool grants, connect accounts or change a pending approval. The current Access and approvals panel handles tools; action rules and own identities remain separate proposed slices until implemented.

The current active-skill rendering reads the body fresh. This workflow-drafting slice does not change skill loading or suspended-turn behavior. If agent-specific skill selection is added, its pending-turn behavior must be designed against the existing suspension record before implementation.

## Actual-data web walkthrough

1. Open **Tools → Skills** (`/tools/skills`) and choose **Draft from workflow**.
2. Enter the workflow name and goal, add the ordered steps, set when the agent should stop or ask for approval, and select an available model.
3. Generate the draft. Review and edit the `SKILL.md` in the existing editor. Cancel discards it; **Save** stores it as a normal personal Skill.
4. Use the saved skill in a conversation or eligible agent where it is available. Set tool access and approval rules through their existing controls.

Product text uses the six Fluent catalogues. The UI reuses the existing daisyUI modal, searchable model selector and Skill editor. Browser verification should cover keyboard/mobile use, cancellation, model refusal, generation errors and explicit saving.

## State-based acceptance tests

- Unauthenticated requests are refused; disabled personal-skill storage returns an actionable error.
- Input bounds and required fields are checked server-side. A model outside the caller's current offered chat models is refused before any model call.
- Generation uses the selected model, caller's current model access and spend limits, has finite byte/token/time caps, and cannot execute tools.
- A successful call returns a valid unsaved manifest; it does not create a file. Only the existing save endpoint writes it.
- Generation errors leave the entered dialog values available. The draft is only transient until the user saves it.

## Remaining implementation scope

Conversation observation selection and safe field projection, private agent skill selection in the UI and runtime, organization-owned Skill sharing, and immutable skill guidance during a suspended action are separate gaps. They are not represented as available controls in the current workflow-drafting dialog.
