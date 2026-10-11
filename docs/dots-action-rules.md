# Dot action rules

## Status and verified gap

Adopted proposal for a later vertical slice, not implemented behavior. The user requires configurable autonomy per tool and per action. Current personal Dots expose a selected-tool list and `Allow` or `Ask before each call` for each tool. Deselection denies the whole tool. There is no argument-sensitive action rule or action catalogue in the tool interface.

Verified sources:

| Source | Current behavior |
|---|---|
| `aiplane-runtime/src/agents/spec/model.rs` | `ToolResource` contains tool binding, permission and approval timeout. Permission supports `always_allow` and `always_ask`. |
| `aiplane-runtime/src/agents/spec.rs` | Spec validation recognizes those resource fields and permissions; it does not validate action rules. |
| `aiplane-runtime/src/agents/approval.rs` | `Permissions` maps tool ID to approval setting; gates wrap entire tools in `AskFirst`. Personal Dot default asks unless `Tool::read_only()` is positive. |
| `aiplane-runtime/src/persona/live.rs` | `LivePersonalDotTool` reloads current Dot state/access and applies the existing personal Dot gate before invoking the live tool. |
| `aiplane-runtime/src/server/tools/ask_first.rs` | Existing durable approval wrapper preserves the tool schema, pauses an available call and accepts the recorded allow-once decision. |
| `aiplane-runtime/src/server/tools/mod.rs` | Tool interface has schema, execution, sensitivity, state-change and read-only declarations, but no action classifier/catalogue. |
| `aiplane-runtime/src/server/tools/sandbox/browse.rs` | Typed `BrowseAction`, validated arguments and schema `action` enum: `navigate`, `read`, `click`, `fill`, `screenshot`, `back`, `console`, `close`. |
| `aiplane-tools/src/browser_control.rs` | Batched `actions[]`, typed parsing and actual schema actions: `navigate`, `go_back`, `read_page`, `find`, `click`, `hover`, `drag`, `type_text`, `press_key`, `scroll`, `screenshot`, `set_viewport`, `wait_for`, `list_tabs`. |

Action strings in output data, such as a document result saying `added` or `replaced`, are not selectable input actions. An arbitrary JSON schema enum is not proof of an action's effect or a safety classification. Named MCP tools already distinguish many provider operations through their actual tool IDs, and continue to use existing tool rules without inventing another action layer.

## Shared mechanisms

Extend the existing typed `ToolResource`, spec validator, `Permissions`, live Dot gate and available-tools API. Reuse `AskFirst`, `chat_turn_suspensions`, Inbox, existing timeout/decision handling and the recorded pending tool arguments. Reuse action parsing owned by each real tool and the existing Access and approvals disclosure. No action-policy table, duplicate approval store, arbitrary JSONPath matcher or generic condition language is introduced.

Add one concrete tool action contract in runtime, first implemented by `browse_page` and consumed by the permission gate and access API in the same change. It provides a trusted catalogue and classifies a validated invocation into known action IDs. Other tools have no action contract unless their implementation supplies one with actual callers/tests. Wrappers must forward this metadata rather than hide it.

## Minimum typed extension

Add `ToolResource.actions`, a map keyed by actual action ID with an enum rule `allow`, `ask` or `deny`. Keep the existing whole-tool permission and timeout as the fallback. This nested field lives in the existing persisted `AgentSpec`; it requires no new database collection. Validation rejects duplicate JSON keys through the existing parsing/validation path or an explicit duplicate check, unknown action IDs and unsupported rules. Actions may be configured only on currently granted/available selected tools exposing the trusted action contract.

The tool supplies an action catalogue from the same typed action definitions used by parsing and schema generation. Refactor current action IDs and their existing schema descriptions into that shared source so UI, schema and execution cannot disagree. Show actual IDs and descriptions. Do not infer new categories like “safe browsing” or generic “send email” from a connector title.

For `browse_page`, classification uses the existing validated `BrowseArgs`/plan parser, including required URL/element/text validation, and returns exactly one `BrowseAction`. An unsupported or malformed value fails before approval or execution. Do not classify by a loose extraction of `args.action` while execution uses different defaulting/coercion.

For batched `browser_control`, classify all parsed actions before executing any step. Deny wins over the entire batch; otherwise any action requiring approval causes one approval for the entire original batch. Execute no allowed prefix before asking about a later step. Whole-batch approval preserves the existing invocation/arguments and avoids a separate per-step suspension engine. This browser tool still depends on its real connected user browser; action rules do not make it an unattended computer.

## Resolution and safety

Resolve one effective decision through the existing gate:

1. Current owner/Dot active state, grants, selected tool and connected identity must permit the call. A missing grant or deselected tool denies every action.
2. Parse and classify the complete invocation using trusted implementation data.
3. A saved explicit action rule overrides that tool's fallback rule for that action. Missing action rules inherit the existing saved tool permission; without a saved permission, retain the personal Dot default of asking for unknown effects.
4. Intersect with existing connector modes and mandatory platform restrictions. An explicit action Allow cannot override connector Ask/Off, a revoked tool or a platform prohibition.
5. Deny refuses before execution; Ask uses the existing approval wrapper and timeout; Allow invokes the same actual tool implementation.

The owner can explicitly permit `read` while leaving other `browse_page` actions on Ask, or deny `fill`. These are user decisions, not automatically derived risk labels. Navigation/back can trigger requests and page code, and filling may submit; an enum name does not establish read-only safety. The initial slice introduces no automatic reduction of approval for action names. Existing positively read-only tool declarations retain their current tool-level meaning.

Tool rules remain the fallback rather than an additional hard ceiling that makes an action override ineffective: the user can set a conservative tool default and explicitly allow an individual action. The owner's grants, connector restrictions and platform rules remain hard ceilings. Explain this relationship in the UI and tests so “Allow read” means what it says.

## Pending-call and identity preservation

Approval applies only to the original tool ID and immutable stored arguments, including every step of a batch. On resume, reclassify those same arguments with the trusted current implementation and reload current grants, tool selection, action rules and active state. A now-denied action cannot execute on an earlier approval. A valid allow-once decision can settle the pending call when the current rule is Ask; it does not rewrite saved rules.

Preserve the identity/generation pinning specified in [dots-identities.md](dots-identities.md). A pending call cannot migrate to a newly selected provider account or replacement credential. Action resolution must sit inside the live revalidation path, before a resumed tool receives its allow-once context. Do not let an outer generic wrapper consume approval before current policy/identity has been checked.

Store the classified action IDs with the pending invocation facts when the action contract is introduced. Resume verifies the current parser classifies the stored arguments into the same action set/order; incompatible implementation changes refuse with an actionable request for a new invocation. This metadata belongs to the existing pending-call encoding, not an independent approval record. It contains IDs, not copied secrets or page content. Exact identity pinning is a prerequisite to claims about changed accounts; that proposal is not current behavior.

## API and actual-data web walkthrough

Extend existing `/api/v0/dots/{id}/access` rows with nullable action catalogue data and saved action rules. PUT replaces the selected tools and their nested action rules through the existing owner-scoped audited spec save, preserving unrelated spec fields. Inventory is resolved from current tools; no action list is invented for unsupported MCP or built-in tools. Inactive Dots retain editable settings under the existing owner checks.

Walkthrough: expand **Access and approvals**, select actual `browse_page`, choose **Ask before each call** as its tool default and open an **Actions** disclosure supplied by that tool's catalogue. Each row displays the actual action ID and existing description plus **Use tool rule**, **Allow**, **Ask**, or **Deny**. Choose Allow for `read` and Deny for `fill`; save through the existing save/error/reload flow. The tool row shows the actual number of explicit overrides derived from saved data. It does not display a fabricated safety badge.

The UI explains that missing overrides use the tool rule and connector restrictions still apply. A batch asks once if any included action requires it and is denied if any is prohibited. Tool/connector identity stays visible using existing real connection metadata. Unknown/stale actions are shown as unavailable rather than dropped silently; saving requires correction/reload and never turns a removed denial into Allow. The server repeats availability and schema checks.

Reuse the current native disclosure, select controls, loading/error states and daisyUI classes. All added strings go through six Fluent catalogues. The existing named MCP action rows remain unchanged; they do not gain empty nested action controls. Coordinator browser verification must include keyboard/mobile behavior and pending-rule revocation before the feature is reported complete.

## Implementation order and tests

First add the typed action catalogue/classifier contract, `browse_page` concrete implementation and spec/permission tests. Then integrate live revalidation and pending metadata, followed by access API/UI and optional batched browser-control support. Coordinate ownership: one agent on typed spec/permissions/tool contract, one on concrete tools/live resume, one on access API/UI/catalogues after contracts are fixed. Shared protocol/spec and generated files each have one writer. No Rust builds run concurrently against the shared target.

Tests use actual parsers, existing in-memory SQLite approval rows and fake/mocked remote collaborators with finite input:

- Current specs with no action map preserve behavior. Unknown actions/rules, duplicate IDs, unsupported tools and stale inventory are rejected. Schema enum, classifier and catalogue match the same implementation source.
- Explicit allow/ask/deny and fallback resolution produce observable execution/refusal/suspension; unknown tool effects remain Ask and grants/connector Off or Ask cannot be bypassed.
- `browse_page` malformed/missing action arguments fail before side effects. Different valid actions use their explicit rules without granting arbitrary code or unrelated browser tools.
- A mixed batch with Deny executes nothing; a batch with Ask suspends once with its complete original arguments; an approved valid batch executes in its existing order. No prefix executes before suspension.
- Deactivation, grant revocation, tool removal or new action Deny between suspension and approval prevents execution. Stored action mismatch after implementation change refuses safely.
- Different connected account/generation on resume is rejected after the identity slice exists; unchanged identity and permitted stored arguments can settle normally.
- API owner isolation and audited nested spec save preserve unrelated bindings/timeouts. UI fixtures show actual action metadata, inherited rules, unavailable actions, errors and accessible mobile controls.

## Constraints still to verify

The current generic approval wrapper does not validate arguments before asking; classification must be introduced before it without losing the live gate. Existing pending encoding needs an exact compatible action-facts field chosen with the runtime owner; no second suspension store is acceptable. External MCP schema enums alone are insufficient to offer per-action rules; only a verified MCP action contract could extend this later. Browser navigation's effects depend on the destination, so this slice does not promise URL-based or argument-sensitive safety categories. Critical platform-wide prohibitions and organization approval authority remain separate policies and cannot be guessed from action names.
