# Conversational Dot setup and participation in chat

## Status

This is a design proposal for the requested conversational setup and shared
chat experience. It does not describe an implemented Dot creation assistant or
an implemented chat mention feature. The complex user flow requires the
maintainer's mockup approval before implementation.

The existing agent architect can converse, inspect resources, propose setup,
create ordinary agent drafts, update drafts and run a test turn. The existing
setup wizard still asks users to apply proposal sections separately. Neither
mechanism establishes a complete conversational Dot lifecycle with ownership,
connector authentication, automation and communication consent.

## Confirmed requirements

- Users describe an intended assistant in their own words. The assistant asks
  targeted questions only for missing information and accepts free text.
- Create and reconfigure personal and organization Dots through the same
  experience. A request to connect an existing company agent is a configuration
  change, subject to the existing communication permissions.
- Infer the available model, tools, selected accounts, action rules, knowledge,
  memory, workspace and automation from actual resources and the user's goal.
  Continue to offer the existing settings for later fine tuning.
- Guide required OAuth or other authentication through the existing provider
  flow, then resume the setup conversation after verified completion.
- A personal Dot can use its owner's account or a selected own account.
  Organization Dots require their own explicitly selected identity.
- External writes require the configured approval rule. Conversational setup
  cannot manufacture permissions, provider accounts or user credentials.
- Users can address an actual permitted Dot from any ordinary chat. Its answer
  is attributed to that Dot in the same conversation. Permitted delegation to
  other Dots uses the existing request/reply execution machinery.
- A conversation can include multiple named Dots. Participation does not grant
  access to another Dot's private memory, tools or credentials.
- Company knowledge Dots check the requester's actual authority before using
  their own permitted knowledge. A message claiming authorization is not proof.
- The active Dot limits remain the existing administrator settings. Incomplete
  planning conversations must not consume active Dot slots.

## Verified reuse points

| Existing source | Mechanism to extend |
|---|---|
| `aiplane-api/src/pages/architect/{mod,tools}.rs` | Existing conversational persona, tool registry, inspection, proposal, draft update and test turn |
| `aiplane-agents/src/db/architect_sessions.rs` | Existing association between a person-owned conversation and the configured agent |
| `web/src/lib/components/agents/ArchitectChat.svelte` | Existing conversation controller, voice entry, ordinary chat submit and live updates |
| `web/src/lib/agent-workspace.svelte.ts` | Draft buffer, staged grants, validation, save, publication and error presentation |
| `web/src/lib/components/agents/setup/StartStep.svelte` | Scenario-based `assist/suggest`, with proposal parts and dropped items |
| `web/src/lib/{dots,organizations}.ts` | Existing personal and organization Dot lifecycle APIs and active limits |
| `web/src/lib/components/dots/DotConnectionsPanel.svelte` | Actual connector inventory and selected own/user identities |
| `web/src/lib/components/dots/DotMemoryPanel.svelte` | Existing Dot and organization memory APIs with actual scope and write authority |
| `docs/dots-background.md`, `docs/dots-webhooks.md` | Existing schedules, webhooks, pinned versions and private run history |
| `docs/dots-communication.md`, `docs/dots-cross-organization.md` | Directed mutual consent, existing delegation, chain limits, budgets, durable waiting and live revocation checks |
| `web/src/lib/Conversation.svelte` | Existing composer, messages, tool activity, decisions and conversation controller |

The live setup walkthrough confirms that an organization Dot currently shows
eligible OAuth connectors before a user selects their tools. An unrelated
connector can therefore show an unavailable-connection warning during a
public-documentation-only setup. The proposed setup summary must distinguish
a required connection from an eligible but unused connector.

## Shared mechanisms

Reuse the architect persona and conversation controller, the typed `AgentSpec`
and actual grantable resources, the ordinary model catalog and routing,
`side_call` metering, existing owner/share checks, Dot lifecycle admission,
connector OAuth and generation pinning, existing memory and automation APIs,
draft revisions, `forward_request`, directed consent, `RunChain`, shared
budgets, `chat_turn_suspensions` and Inbox. Use the existing Modal, chat
components, daisyUI and six Fluent catalogs.

Do not introduce a second model catalog, connector catalog, settings store,
approval queue, run dispatcher or memory store. Any proposed common contract
must have a real setup or chat caller in the same implementation slice.

## Setup flow

1. **Describe.** The existing Dot list starts a planning conversation. The user
   describes a new Dot or selects an existing Dot to change. Actual ownership
   and available organization choices come from the signed-in user's access.
2. **Resolve.** The assistant reads the current draft and resources before
   proposing changes. Missing or ambiguous resources lead to a targeted
   question. An irrelevant knowledge collection is not selected merely because
   it exists. Do not replace absent resource metadata with invented labels.
3. **Connect.** Show only prerequisites for selected capabilities. An OAuth
   action opens the existing provider flow for the explicit credential owner;
   verified connection status resumes the same plan. The assistant does not
   receive passwords or OAuth tokens in conversation text.
4. **Review.** Show the intended name, owner, purpose, selected model, actual
   knowledge/tools, identities, write rules, automation and communication peers.
   Show unresolved requirements and changes from the existing configuration.
   The user can request corrections in the conversation.
5. **Apply.** The user's setup action applies the reviewed configuration through
   existing authorized services and confirms actual persisted results. For an
   organization Dot, make the reviewed runnable version through the existing
   publication path. Report a partial application accurately and preserve the
   conversation for recovery; do not claim an OAuth authorization was undone.
6. **Fine tune.** Details expose the current settings. The same conversation
   assistant can propose a change, with current-state checks before applying.

Creation waits until the reviewed configuration is ready. Admission checks the
current active limit atomically. A conversation describing an unfinished Dot
is not itself an active Dot. Rights required for a personal Dot come from
personal Dot management; do not impose ordinary agent-management rights on
that flow. Organization configuration retains member/share checks.

Planning state belongs to the existing architect conversation and draft
mechanisms. Extend their ownership context if necessary. The applied data
continues to live in `AgentSpec`, existing grants, identity selections,
communication edges, memories and automation records. Revalidate resource
revisions and credential generations at application; never apply a proposal
against a replaced provider account silently.

## Dot mentions and shared conversations

Typing `@` offers actual Dots the caller may invoke. Selection uses stable
resource IDs; names are display data and cannot authorize a call. A mention in
quoted text, a tool result or model output is not a new user invocation.

Before submission, the UI identifies the recipient and the task/context being
shared. Reuse the current explicit-task boundary; private histories and memory
are not added implicitly. The Dot runs with its own permissions and selected
account, and its result appears as that named participant's reply. A participant
has no general right to act as another participant.

Further delegation reuses directed consent and current authority checks,
including at resumed approvals and result delivery. Preserve bounded context,
depth/cycle checks, shared budgets and cancellation. A multi-Dot conversation
does not create an unbounded autonomous reply loop.

Keep session-core owner-agnostic. If attributed participant output requires
protocol metadata, use a generic speaker identity contract there and map Dot
ownership in the agent/runtime layer.

Personal same-owner and personal-to-organization delegation are existing
foundations. Organization-to-organization communication is an implementation
gap and must extend that machinery before the two organization demonstration
Dots can call each other. Do not report selectable peers as authorized solely
because they belong to one organization.

## Implementation order and acceptance

1. Extend the existing architect to review and create a personal Dot from one
   description, with actual resources and existing limits. Approve the mockup
   first, then verify this complete slice in the real authenticated browser.
2. Extend the same flow to organization ownership and existing-Dot changes,
   required OAuth connections, selected accounts and action approvals.
3. Apply reviewed schedule/webhook and communication configuration through
   the existing APIs. Cover stale drafts, revoked rights, changed identities
   and partial application.
4. Extend directed consent and delegation to organization peers. Verify
   isolation, budgets, suspension, revocation and result attribution.
5. Add ordinary-chat mentions and bounded multi-Dot participation using those
   completed shared mechanisms. Approve its interaction mockup before UI work.
6. Complete the separate memory-curation and remaining organization workspace
   requirements documented in their existing designs. Conversational setup
   must describe their actual availability until implemented.

End-to-end checks must include natural-language creation and reconfiguration,
missing resources, provider cancellation/return, default active limits,
per-action writes, own versus user identities, stale proposals, revoked
memberships, permitted/denied delegation, mention-name collisions, attributed
replies and full keyboard operation. Product documentation describes each
slice only after its behavior is implemented and verified.
