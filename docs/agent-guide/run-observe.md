# Publish channels, answer waiting work and inspect activity

A published agent can serve a website widget and authenticated Agent-to-Agent callers. Its Inbox handles persisted waiting turns. Insights separates aggregate behavior from detailed activity records.

Managing channels, keys and publish policy requires agent-management permission and write access. Responding to inbox work uses the agent's sharing permissions; it does not require giving every responder builder access.

Organization Dots currently offer private saved-draft Test chat for current
member writers. Decisions for those version-0 conversations are answered in
the test chat and are excluded from the Inbox. Their published conversations,
website/A2A channels, notifications and automation are unavailable. The channel
and Inbox instructions below describe ordinary published agents.

## Embed an agent on a website

1. Publish the agent and verify its model/tool grants.
2. Configure permitted site origins in **Setup → Site** or the advanced publish settings.
3. Open **Settings → Sharing** (`/agents/<id>?tab=settings&sub=sharing`) and find the embed-keys card.
4. Create a named key with the exact website origins that may use it.
5. Copy the generated snippet immediately and add it to that site.
6. Open the site and test a real visitor conversation, including any waiting/handoff path.

The snippet loads the installation's `/embed.js`. The plaintext key is shown once during creation; the list later shows key name/origins, creator/date and revoked state. Revoke a key to disable that credential. Create a replacement if the original snippet was lost.

The setup Site step also creates a key/snippet from the configured origins and previews widget appearance. Choose the widget color there. Voice input/output are separate toggles with transcription/speech model choices and an optional speech voice. Each enabled direction needs a model grant; the assistant stages eligible grants on save. When no explicit voice model is selected, the corresponding gateway default is used. Verify microphone permission, model availability and the offered speech voice before publishing voice controls.

The key's origin list and the published spec's optional origin list both constrain admission. A spec with no origin list does not add another restriction; the key still supplies its origin policy. Origins contain scheme, host and optional port, not an entire page path. The widget uses browser session storage and is not the user's signed-in AIplane chat session.

### Visitor lifetime, rates and retained data

The publish policy can configure idle lifetime, conversation retention, audit retention, visitor/IP rates and owner budget. Defaults are a 30-minute idle lifetime, a 24-hour absolute visitor-session cap, 20 messages per visitor per 10 minutes and 60 conversation starts/messages per client IP per 10 minutes. Conversation retention defaults to 30 days and activity-log retention to 365 days.

The IP allowance is shared by callers behind that IP. A2A and widget traffic feed the shared inbound rate accounting where their scopes apply. Publish budgets supplement operator limits; unset owner budgets rely on those operator limits.

Conversation expiry and stored-data retention are different: a visitor token becoming unusable does not immediately delete its stored conversation. The retention sweeper removes expired conversation data and later removes eligible activity chains according to their own retention. Audit retention must not be shorter than conversation retention.

Output-filter patterns identify values that need provenance checks in an answer. The configured action is `withhold` (the default) or `redact`: withhold replaces the answer with a fixed fallback; redact replaces offending identifiers and delivers the rest. Test known tool-backed identifiers and invented ones. This checks traceability in output, not provider contracts.

## Use A2A

Enable A2A in the agent's publish settings and publish the version to serve its public card at `/a2a/agents/<id>/agent-card.json`. The **test and publish guide** explains the publish setting. A2A messaging at `/a2a/agents/<id>` requires a gateway-minted **system-principal** bearer token (`gws_…`) whose principal holds the `a2a_caller` grant for this agent. A person's API token, browser cookie or embed key cannot call this surface. To authorize a caller, create a system principal, grant it `a2a_caller` for this agent, and issue a token for that principal; the [HTTP API reference](../reference/api.md) lists the principal and A2A endpoints. The grant is scoped to this agent.

The A2A surface supports message/task operations, streaming and cancellation through its implemented protocol. Keep context/task IDs from responses to continue or inspect the corresponding work. When integrating a remote agent route, configure its endpoint, authentication and task timeout in the agent spec, then test completion and failure handling. Outbound remote calls use the guarded network client; a URL that is refused must be corrected rather than bypassing the refusal.

## Answer waiting work

Open **Inbox** (`/inbox`). Items identify the agent or conversation, request kind, creation time, optional inbox label, relevant context and expiry.

| Kind | What the responder supplies |
|---|---|
| Approval | Allow this call once or deny it, using the offered actions |
| Secure input | The requested value through its input control |
| Human answer | An answer to the handoff question |

1. Read the waiting card's question and context. For approval, inspect the tool arguments/preview.
2. Open the linked conversation or agent when more context is needed and your rights permit it.
3. Submit the offered decision or value before expiry.
4. Check success and the resumed operation.

The shared suspension card is also used in normal chat and builder tests. A decision settles that waiting request; it does not permanently grant a tool. Another responder may settle it first, in which case the Inbox reports that it is already settled and refreshes. Expired or already answered items cannot be answered again.

Read/write shares include responder access; a respond-only share permits inbox work without exposing the agent builder. Handoff configuration decides whether to include transcript and state context. Sensitive state can identify its trusted writer without disclosing the value to the model.

### Notify responders

The agent's Sharing tab has Slack and Discord incoming-webhook channels. Add the channel kind, name, webhook URL, language and optional details flag. The server seals the URL and shows its host instead of the credential. With details off, notifications carry the agent, request kind and inbox link; enabling details shares more handoff context with that external channel. Remove a channel when its destination changes.

Browser push is another notification path, requiring global push configuration and the responder's browser permission. A notification announces waiting work; the actual decision is submitted through the authorized Inbox.

## Inspect analytics

Open **Insights → Analytics** (`?tab=insights&sub=analytics`). Choose a time range and optional published version.

The view counts conversations, turns, sub-agent dispatch/completion, gate refusals, output blocks, limit refusals, human handoffs, model requests/tokens and priced cost. It has a daily chart and breakdowns by route, missing slot, incomplete reason, output action and limit kind.

Analytics contains counts rather than visitor message text. Builder test chats are excluded. Version-filtered views explain how unversioned records are handled. Monetary totals depend on configured model prices, and absence of a cost tile is not evidence of free upstream usage.

## Inspect and export activity

Open **Insights → Activity** (`?tab=insights&sub=activity`). Filter by conversation, event group and dates. Expand an event for its actual detail; load more to continue through the paginated log. A single-conversation view groups events by turn.

Activity includes model exchanges, tool calls, state writes, routes, pauses and changes to the agent. Export downloads the filtered records. Verify checks the stored hash chains and reports the event/chain counts, broken chain if any and unanchored records when present. Verification of those chains is distinct from proving the correctness of every recorded action.

Activity details can include conversation and tool data. Grant read access and handle exports accordingly. Conversation retention and activity retention can differ, so a log can outlive the conversation it describes.

## Troubleshooting

| Symptom | Check |
|---|---|
| Widget rejected | Key revoked, exact origin, spec origin restriction, published/enabled agent |
| Visitor must restart | Idle/absolute expiry or browser tab session |
| Shared office hits rate limit | Combined IP traffic and publish/operator limits |
| A2A unauthorized | System-principal bearer token and its `a2a_caller` grant |
| Inbox empty for responder | Shares, still-waiting requests and expiry |
| Decision says already settled | Another answer or expiry; refresh |
| Notification arrives without detail | Channel's details setting |
| Analytics stays empty after test chat | Builder tests are excluded |
| Activity verification reports broken chain | Preserve the report/export and investigate stored records |
