# AIplane feature comparison

This is a feature-by-feature comparison of croit AIplane, LiteLLM and Open WebUI. It is maintained as a standalone product document; the README remains focused on AIplane.

**Checked on 6 October 2026.** AIplane entries use the local `main` implementation revision pinned at the end of this document. LiteLLM and Open WebUI entries describe the public product documentation linked in the source index. For LiteLLM, ✅ means available in the open-source edition; 🟡 Enterprise means the capability requires a paid Enterprise license. Product capabilities change; recheck those sources before using this matrix in a proposal or contract.

The request-priority rows were checked on **11 October 2026** against AIplane revision `d5b81c2a` and the official documentation cited as L31 and O6. Other entries retain the review dates and revision pins below.

## How to read the matrix

| Mark | Meaning |
|---|---|
| ✅ | The feature is directly documented and available in the product. |
| 🟡 | The feature exists with a material limitation, configuration prerequisite, external service, plugin, or edition dependency. For LiteLLM, the note identifies Enterprise-only capabilities. Read the linked evidence. |
| ❌ | The reviewed product documentation explicitly says the feature is unavailable or the product surface does not provide it. This is a scoped product comparison, not a claim that no workaround exists. |
| ⚪ | Not established by the reviewed sources. Treat this as unknown, not as a product gap. |
| — | Not applicable to the product's role in this row. |

“Partial” is not a score. It means that the feature's scope or operating conditions differ. A green LiteLLM mark means its OSS edition; it does not include Enterprise capabilities. A green mark does not mean the implementations are equivalent in quality, scale, security posture, or support. The matrix compares documented product behavior, not marketing claims, and does not infer a feature from a protocol alone.

## Product scope and model access

| Feature | croit AIplane | LiteLLM | Open WebUI | Evidence |
|---|:---:|:---:|:---:|---|
| Self-hosted deployment | ✅ | ✅ | ✅ | A1 L1 O1 |
| Browser-based employee workspace | ✅ | 🟡 | ✅ | A2 L1 O1 |
| Centralized model gateway for applications | ✅ | ✅ | 🟡 | A3 L1 O1 |
| Gateway and employee workspace in one product | ✅ | 🟡 | 🟡 | A2 A3 L1 O1 |
| OpenAI-compatible client interface | ✅ | ✅ | ✅ | A3 L2 O1 |
| Anthropic Messages-compatible client interface | ✅ | ✅ | 🟡 | A3 L2 O1 |
| Python client SDK for application developers | ⚪ | ✅ | ⚪ | L3 |
| Native model-provider adapters | 🟡 | ✅ | 🟡 | A4 L4 O1 |
| OpenAI-compatible upstream backends | ✅ | ✅ | ✅ | A4 L4 O1 |
| More than 100 provider integrations advertised | ⚪ | ✅ | ⚪ | L4 |
| Ollama integration | 🟡 | ✅ | ✅ | A4 L4 O1 |
| Hosted model services through compatible upstreams | 🟡 | ✅ | 🟡 | A4 L4 O1 |
| Self-hosted inference services through compatible upstreams | ✅ | ✅ | ✅ | A4 L4 O1 |
| Anthropic-native upstream support | ✅ | ✅ | 🟡 | A4 L4 O1 |
| TypeSafe System One upstream support | ✅ | ⚪ | ⚪ | A4 |
| OpenAI Chat Completions endpoint | ✅ | ✅ | ✅ | A3 L2 O1 |
| OpenAI Responses endpoint | ✅ | ✅ | 🟡 | A3 L28 O24 |
| Anthropic Messages endpoint | ✅ | ✅ | ✅ | A3 L2 O24 |
| Embeddings endpoint | ✅ | ✅ | 🟡 | A3 L2 O24 |
| Image generation endpoint | ✅ | ✅ | ⚪ | A3 L28 |
| Image editing endpoint | ✅ | ✅ | ⚪ | A3 L28 |
| Audio transcription endpoint | ✅ | ✅ | ⚪ | A3 L28 |
| Text-to-speech endpoint | ✅ | ✅ | ⚪ | A3 L28 |
| Batch inference endpoint | ❌ | ✅ | ⚪ | A3 L28 |
| Public reranking endpoint | ✅ | ✅ | ⚪ | A3 L28 |
| Public moderation endpoint | ❌ | ✅ | ⚪ | A3 L28 |
| OpenAI-compatible streaming | ✅ | ✅ | ✅ | A3 L2 O1 |
| Provider-specific request options pass through where supported | 🟡 | ✅ | 🟡 | A3 L2 O1 |

## Gateway routing and resilience

| Feature | croit AIplane | LiteLLM | Open WebUI | Evidence |
|---|:---:|:---:|:---:|---|
| Multiple configured model backends | ✅ | ✅ | ✅ | A5 L5 O2 |
| Model pools or groups | ✅ | ✅ | 🟡 | A5 L5 O2 |
| Pool-level backend membership | ✅ | ✅ | 🟡 | A5 L5 O2 |
| Load balancing across backends | ✅ | ✅ | 🟡 | A5 L5 O2 |
| Round-robin selection | ✅ | ✅ | ⚪ | A5 L5 |
| Least-in-flight selection | ✅ | ✅ | ⚪ | A5 L5 |
| Prompt-prefix affinity selection | ✅ | 🟡 | ⚪ | A5 L5 |
| Backend health probes | ✅ | ✅ | ⚪ | A5 L5 O2 |
| Per-backend in-flight limits | ✅ | ✅ | ⚪ | A5 L5 |
| Priority queue for model requests | ✅ | 🟡 Beta; testing only | ⚪ | A36 L31 O6 |
| Administrator-assigned Low/Medium/High priorities on groups and API tokens | ✅ | ⚪ | ⚪ | A36 L31 O6 |
| Translate trusted priorities into backend scheduler parameters | 🟡 Requires backend support and enabled scheduling | ⚪ | ⚪ | A36 L31 O6 |
| Automatic route selection by quality, balance, or cost | ✅ | 🟡 | 🟡 | A6 L5 O3 |
| Stable model aliases | ✅ | ✅ | ✅ | A5 L5 O2 |
| Unknown-model fallback | ✅ | ✅ | 🟡 | A5 L5 O2 |
| Offline-capacity fallback | ✅ | ✅ | ⚪ | A5 L5 |
| Retry another replica after eligible pre-output failures | ✅ | 🟡 | ⚪ | A7 L5 |
| Wait for a known backend to recover within a configured budget | ✅ | ⚪ | ⚪ | A7 |
| Retry-After response when gateway capacity is exhausted | ✅ | ✅ | ⚪ | A7 L5 |
| Shadow rollout for automatic routes | ✅ | 🟡 | ⚪ | A6 L6 |
| Route decision history and outcome inspection | ✅ | ✅ | ⚪ | A6 L6 |
| Route session affinity | ✅ | ✅ | ⚪ | A6 L6 |
| Traffic mirroring to a silent model | ⚪ | ✅ | ⚪ | L6 |
| A/B model experiments | 🟡 | ✅ | ✅ | A6 L6 O4 |
| Cost-aware model choice | ✅ | ✅ | 🟡 | A6 L5 O3 |
| Provider/model capability catalog | ✅ | ✅ | ✅ | A5 L4 O2 |
| Per-model capability overrides | ✅ | 🟡 | ✅ | A5 L5 O2 |
| Global model defaults | ✅ | ✅ | ✅ | A5 L5 O2 |

## Identity, access and governance

| Feature | croit AIplane | LiteLLM | Open WebUI | Evidence |
|---|:---:|:---:|:---:|---|
| Multi-user access | ✅ | ✅ | ✅ | A8 L7 O5 |
| OIDC single sign-on | ✅ | 🟡 (free up to 5 users; Enterprise above that) | ✅ | A8 L29 O5 |
| End-user local account/password sign-in | ❌ | — | ✅ | A8 O5 |
| Basic username/password protection for the admin UI | ⚪ | ✅ | ✅ | L29 O5 |
| LDAP authentication | ⚪ | ⚪ | ✅ | O5 |
| SCIM user provisioning | ⚪ | 🟡 Enterprise | ✅ | L29 O5 |
| Identity-provider OIDC/JWT authentication | ✅ | 🟡 Enterprise | ✅ | A8 L29 O5 |
| Map identity-provider groups to product permissions | ✅ | ⚪ | ✅ | A8 L7 O5 |
| Basic global proxy roles and user/team/key controls | ✅ | ✅ OSS | ✅ | A8 L7 O5 L29 |
| Organization/team-scoped delegated administrator roles | ✅ | 🟡 Enterprise | ⚪ | A8 L7 L29 O5 |
| Fine-grained team-member permissions | ✅ | 🟡 Enterprise | ⚪ | A8 L7 L29 O5 |
| Per-model access restrictions | ✅ | ✅ | ✅ | A8 L7 O5 |
| Per-tool access restrictions | ✅ | ✅ | ✅ | A8 L7 O5 |
| Per-agent access restrictions | ✅ | 🟡 | 🟡 | A9 L8 O5 |
| Per-knowledge-collection access restrictions | ✅ | 🟡 | ✅ | A10 L8 O5 |
| Personal API tokens | ✅ | ✅ | ✅ | A8 L7 O5 |
| Revocable API tokens | ✅ | ✅ | ✅ | A8 L7 O5 |
| Token model restrictions | ✅ | ✅ | 🟡 | A8 L7 O5 |
| Token tool restrictions | ✅ | ✅ | 🟡 | A8 L7 O5 |
| Token request quotas | ✅ | ✅ | ⚪ | A8 L9 |
| Token token-volume quotas | ✅ | ✅ | ⚪ | A8 L9 |
| Token cost quotas | ✅ | ✅ | ⚪ | A8 L9 |
| Group/role usage limits | ✅ | ✅ | ⚪ | A8 L9 |
| Per-user usage limits | ✅ | ✅ | ⚪ | A8 L9 O6 |
| Per-agent/system-principal limits | ✅ | 🟡 | ⚪ | A8 L9 |
| Global usage limits | ✅ | ✅ | ⚪ | A8 L9 |
| Per-model usage limits | ✅ | ✅ (model-level controls) | ⚪ | A8 L9 |
| Different model budgets for each virtual key | ⚪ | 🟡 Enterprise | ⚪ | L29 |
| Usage reporting by user | ✅ | ✅ | ✅ | A8 L10 O6 |
| Usage reporting by model | ✅ | ✅ | ✅ | A8 L10 O6 |
| Usage reporting by API token | ✅ | ✅ | ⚪ | A8 L10 |
| Usage reporting by request source | ✅ | 🟡 | ⚪ | A8 L10 |
| Cost reporting based on configured prices | ✅ | ✅ | 🟡 | A8 L10 O6 |
| Unpriced usage is visibly identified | ✅ | 🟡 | ⚪ | A8 L10 |
| GDPR coverage declaration per model pool | ✅ | ⚪ | ⚪ | A11 |
| NDA coverage declaration per model pool | ✅ | ⚪ | ⚪ | A11 |
| Content guard can monitor, confirm, or deny a request | ✅ | 🟡 (guardrail framework OSS; behavior varies) | ⚪ | A11 L11 L29 |
| Built-in moderation callback integrations | 🟡 | 🟡 Enterprise for listed integrations | ⚪ | A11 L29 |
| Guardrails scoped to individual keys or teams | ✅ | 🟡 Enterprise | ⚪ | A11 L29 |
| Human approval before a tool action | ✅ | 🟡 | ⚪ | A12 L11 O7 |
| Secure input collection during a workflow | ✅ | ⚪ | ⚪ | A12 |
| Append-only agent activity records with integrity verification | ✅ | ⚪ | ⚪ | A13 |
| Admin impersonation with an audit record | ✅ | ⚪ | ⚪ | A8 |

## Employee workspace and collaboration

| Feature | croit AIplane | LiteLLM | Open WebUI | Evidence |
|---|:---:|:---:|:---:|---|
| Persistent multi-conversation chat | ✅ | 🟡 | ✅ | A2 L1 O1 |
| Streaming chat replies | ✅ | 🟡 | ✅ | A2 L2 O1 |
| Edit and retry a message | ✅ | ⚪ | ✅ | A2 O8 |
| Interrupt a running response | ✅ | ⚪ | ✅ | A2 O8 |
| Queue a follow-up while the model is responding | ✅ | ⚪ | ✅ | A2 O1 |
| Share a conversation | ✅ | ⚪ | ✅ | A2 O1 |
| Fork a conversation | ✅ | ⚪ | ✅ | A2 O8 |
| Export a conversation as Markdown | ✅ | ⚪ | ✅ | A2 O8 |
| Export a conversation as PDF | ✅ | ⚪ | ⚪ | A2 |
| Compare multiple models side by side | ⚪ | ⚪ | ✅ | O1 |
| Shared team conversation channels | ⚪ | ⚪ | ✅ | O9 |
| Threaded replies and reactions in shared channels | ⚪ | ⚪ | ✅ | O9 |
| Chat folders, tags, and pins | 🟡 | ⚪ | ✅ | A2 O1 |
| Personal memory across conversations | ✅ | 🟡 | ✅ | A14 L12 O1 |
| User-editable memory and preferences | ✅ | ⚪ | ✅ | A14 O1 |
| Reusable personal skills/instructions | ✅ | 🟡 | ✅ | A15 L13 O3 |
| Temporary chats | ⚪ | ⚪ | ✅ | O10 |
| Message autocomplete | ⚪ | ⚪ | ✅ | O8 |
| Follow-up suggestions | ⚪ | ⚪ | ✅ | O8 |
| Reasoning/thinking display | 🟡 | 🟡 | ✅ | A2 L2 O8 |
| Multilingual interface | ✅ | ⚪ | ⚪ | A16 |
| Six interface languages documented for AIplane | ✅ | ⚪ | ⚪ | A16 |
| Mobile-responsive chat interface | ✅ | ⚪ | ✅ | A2 O1 |
| Maintained first-party extension controls the user's logged-in browser | ✅ | ⚪ | ❌ | A17 O22 O23 |
| Browser automation available through tools or extensions | ✅ | 🟡 | 🟡 | A17 L16 O3 O15 |
| Browser page navigation, reading, clicking, and typing | ✅ | ⚪ | 🟡 | A17 O3 O15 |
| Browser screenshot of the full page or a selected region | ✅ | ⚪ | 🟡 | A17 O15 O23 |
| Browser control requires explicit user enablement | ✅ | ⚪ | 🟡 | A17 O3 |
| Conversation-linked editable documents | ✅ | ⚪ | 🟡 | A18 O1 |
| Document revision history and restore | ✅ | ⚪ | ⚪ | A18 |
| Generated file downloads from chat | ✅ | ⚪ | ✅ | A18 O1 |

## Files, knowledge and retrieval

| Feature | croit AIplane | LiteLLM | Open WebUI | Evidence |
|---|:---:|:---:|:---:|---|
| Upload files into a conversation | ✅ | ⚪ | ✅ | A18 L1 O1 |
| Read common text and document attachments | ✅ | ⚪ | ✅ | A18 L1 O12 |
| PDF text extraction | ✅ | ⚪ | ✅ | A18 O12 |
| OCR for scanned documents | 🟡 | ⚪ | ✅ | A19 O12 |
| Structured Office document extraction | 🟡 | ⚪ | 🟡 | A19 O12 |
| Generate and edit documents in a canvas | ✅ | ⚪ | 🟡 | A18 O1 |
| Restore an earlier canvas revision | ✅ | ⚪ | ⚪ | A18 |
| Search across configured company knowledge | ✅ | ⚪ | ✅ | A10 O13 |
| Hybrid lexical and vector retrieval | ✅ | ⚪ | ✅ | A10 O13 |
| Optional cross-encoder reranking for retrieval | ✅ | ⚪ | ✅ | A10 O13 |
| Search citations linked to source material | ✅ | ⚪ | ✅ | A10 O13 |
| Git knowledge source | ✅ | ⚪ | ⚪ | A10 |
| WebDAV knowledge source | ✅ | ⚪ | ⚪ | A10 |
| Google Drive knowledge source | ✅ | ⚪ | 🟡 | A10 O13 |
| HyperKitty knowledge source | ✅ | ⚪ | ⚪ | A10 |
| Multiple vector database backends | 🟡 | ⚪ | ✅ | A10 O13 |
| Bring-your-own embedding backend | ✅ | ⚪ | ✅ | A10 O13 |
| Knowledge access by user group | ✅ | ⚪ | ✅ | A10 O5 |
| Agentic retrieval through model tools | ✅ | ⚪ | ✅ | A10 O13 |
| Whole-document context mode | 🟡 | ⚪ | ✅ | A18 O13 |
| Manage uploaded files in a centralized file manager | ⚪ | ⚪ | ✅ | O14 |
| Versioned knowledge source synchronization | ✅ | ⚪ | 🟡 | A10 O13 |
| Knowledge sources limited to a configured allowlist | ✅ | ⚪ | 🟡 | A10 O13 |

## Tools, integrations and extensibility

| Feature | croit AIplane | LiteLLM | Open WebUI | Evidence |
|---|:---:|:---:|:---:|---|
| Built-in web search | ✅ | 🟡 | ✅ | A20 L15 O1 |
| Fetch and read a web page | ✅ | 🟡 | ✅ | A20 L15 O1 |
| MCP client connections | ✅ | ✅ | ✅ | A21 L16 O3 |
| MCP tool gateway/proxy | 🟡 | ✅ | 🟡 | A21 L16 O3 |
| Expose REST/OpenAPI tools | 🟡 | ✅ | ✅ | A21 L16 O3 |
| Per-tool permission grants | ✅ | ✅ | ✅ | A8 L7 O5 |
| Tool discovery for API-token callers | ✅ | 🟡 | 🟡 | A21 L16 O3 |
| Tool calls from OpenAI-compatible model requests | ✅ | ✅ | ✅ | A21 L16 O3 |
| Gateway executes granted server-side tools | ✅ | ✅ | 🟡 | A21 L16 O3 |
| Return client-owned tool calls to the API caller | ✅ | ✅ | 🟡 | A21 L16 O3 |
| Dedicated tool execution sandbox | ✅ | ⚪ | 🟡 | A22 O15 |
| Per-run gVisor isolation for code execution | ✅ | ⚪ | 🟡 | A22 O15 |
| Optional allowlisted sandbox network egress | ✅ | ⚪ | 🟡 | A22 O15 |
| User-browser interaction through a paired extension | ✅ | ⚪ | ⚪ | A17 |
| Headless sandbox browser | ✅ | ⚪ | 🟡 | A22 O15 |
| Create and convert Office/PDF documents through tools | ✅ | ⚪ | 🟡 | A22 O15 |
| Generate QR codes | ✅ | ⚪ | ⚪ | A23 |
| Generate images through configured image backends | ✅ | 🟡 | ✅ | A23 L2 O1 |
| Edit images through configured image backends | ✅ | 🟡 | ✅ | A23 L2 O1 |
| Voice dictation in chat | ✅ | ⚪ | ✅ | A24 O1 |
| Spoken chat / voice mode | ✅ | ⚪ | ✅ | A24 O1 |
| Web Push notifications | ✅ | ⚪ | 🟡 | A23 O3 |
| Connectors with managed OAuth credentials | ✅ | 🟡 | 🟡 | A21 L16 O3 |
| User-managed connector accounts | ✅ | ⚪ | 🟡 | A21 O3 |
| Skills as reusable Markdown instructions | ✅ | 🟡 | ✅ | A15 L13 O3 |
| User-created in-process Python plugins | ⚪ | ✅ | ✅ | L18 O3 |
| Request/response middleware plugin framework | ⚪ | ✅ | ✅ | L18 O3 |
| Community extension catalog | ⚪ | 🟡 | ✅ | L18 O3 |

## Agents and workflow automation

| Feature | croit AIplane | LiteLLM | Open WebUI | Evidence |
|---|:---:|:---:|:---:|---|
| Dedicated business-agent builder | ✅ | 🟡 | 🟡 | A9 L19 O2 |
| Agent-specific system instructions | ✅ | ✅ | ✅ | A9 L19 O2 |
| Agent-specific model selection | ✅ | ✅ | ✅ | A9 L19 O2 |
| Agent-specific knowledge grants | ✅ | 🟡 | ✅ | A9 L19 O2 |
| Agent-specific tool grants | ✅ | ✅ | ✅ | A9 L19 O2 |
| Agent state across runs | ✅ | 🟡 | 🟡 | A9 L19 O2 |
| Agent identity and delegated permissions | ✅ | 🟡 | ⚪ | A9 L19 |
| Repeatable agent test cases | ✅ | 🟡 | ⚪ | A25 L19 |
| Require a passing test suite before agent publication | ✅ | ⚪ | ⚪ | A25 |
| Agent version publication workflow | ✅ | 🟡 | 🟡 | A25 L19 O2 |
| Publish an agent as a website widget | ✅ | 🟡 | ⚪ | A26 L19 |
| Agent-to-agent (A2A) interface | ✅ | ✅ | ⚪ | A26 L28 |
| Remote agent routes | ✅ | ✅ | ⚪ | A26 L28 |
| Durable human approval and handoff | ✅ | 🟡 | ⚪ | A12 L19 |
| Shared inbox for agent decisions | ✅ | ⚪ | ⚪ | A12 |
| Collect secure user input during an agent run | ✅ | ⚪ | ⚪ | A12 |
| Scheduled agent/prompt runs | ✅ | ⚪ | ✅ | A27 O16 |
| Webhook-triggered agent runs | ✅ | 🟡 | 🟡 | A27 L20 O16 |
| Persisted run conversation and history | ✅ | 🟡 | ✅ | A27 L20 O16 |
| Agent run analytics | ✅ | 🟡 | ⚪ | A13 L20 |
| Export agent activity for review | ✅ | ⚪ | ⚪ | A13 |
| Agent activity hash-chain verification | ✅ | ⚪ | ⚪ | A13 |
| Task lists maintained by models | ⚪ | ⚪ | ✅ | O1 O3 |
| Multi-agent orchestration framework in the product | 🟡 | ✅ | 🟡 | A9 L19 O3 |

## Observability, security and operations

| Feature | croit AIplane | LiteLLM | Open WebUI | Evidence |
|---|:---:|:---:|:---:|---|
| Request usage records | ✅ | ✅ | ✅ | A8 L10 O6 |
| Token and cost accounting | ✅ | ✅ | ✅ | A8 L10 O6 |
| Per-backend health visibility | ✅ | ✅ | ⚪ | A5 L5 |
| Per-backend in-flight visibility | ✅ | ✅ | ⚪ | A5 L5 |
| Agent activity audit trail | ✅ | 🟡 | ⚪ | A13 L20 |
| Browser-control action audit trail | ✅ | ⚪ | ⚪ | A17 |
| External logging callbacks | ⚪ | ✅ | 🟡 | L21 O17 |
| OpenTelemetry traces, metrics, and logs | ⚪ | ✅ | ✅ | L22 O18 |
| Prometheus metrics endpoint | ✅ | ✅ | 🟡 | A34 L22 O18 |
| Built-in usage dashboard | ✅ | ✅ | ✅ | A8 L10 O6 |
| Blind model evaluation / arena | ⚪ | 🟡 | ✅ | L23 O6 |
| Model-quality evaluation workflow | 🟡 | ✅ | ✅ | A25 L23 O6 |
| Encrypted stored upstream credentials | ✅ | ✅ | 🟡 | A28 L24 O19 |
| Browser session authentication | ✅ | ✅ | ✅ | A8 L7 O5 |
| API credentials are not forwarded to the model provider | ✅ | ✅ | 🟡 | A3 L24 O19 |
| Secret-manager integrations | 🟡 | 🟡 Enterprise | 🟡 | A28 L24 L29 O19 |
| Automated rotation of virtual API keys | ⚪ | 🟡 Enterprise | ⚪ | L29 |
| IP allowlists for gateway access | ⚪ | 🟡 Enterprise | ⚪ | L29 |
| Public/private route access controls | ⚪ | 🟡 Enterprise | ⚪ | L29 |
| Team-specific log routing and logging opt-out | ⚪ | 🟡 Enterprise | ⚪ | L29 |
| Admin-operation audit logs | ⚪ | 🟡 Enterprise | ⚪ | L29 |
| Multi-region deployment under one license | ⚪ | 🟡 Enterprise | ⚪ | L25 L29 |
| Role/group-aware authorization on application APIs | ✅ | ✅ | ✅ | A8 L7 O5 |
| Public agent visitor rate limiting | ✅ | ⚪ | ⚪ | A29 |
| Per-user/model/token spend budgets | ✅ | ✅ | ⚪ | A8 L9 |
| Shared-state multi-replica application scaling | ❌ | ✅ | ✅ | A30 L25 O20 |
| Docker deployment | ✅ | ✅ | ✅ | A31 L25 O20 |
| Docker Compose deployment | ✅ | ✅ | ✅ | A31 L25 O20 |
| Kubernetes Helm deployment | ✅ | ✅ | ✅ | A31 L25 O20 |
| Podman/systemd Quadlet deployment | ✅ | ⚪ | ⚪ | A31 |
| Terraform deployment modules | ⚪ | ✅ | ⚪ | L25 |
| Source build and self-hosted runtime | ✅ | ✅ | ✅ | A31 L25 O20 |
| Backup and recovery guide | ✅ | 🟡 | 🟡 | A32 L25 O20 |
| Version-matched documentation served inside the installation | ✅ | ⚪ | ⚪ | A33 |
| `llms.txt` and full Markdown manual exports | ✅ | ⚪ | ⚪ | A33 |
| Signed container image verification documented | ✅ | ✅ | ⚪ | A35 L26 |
| Published enterprise support/SLA offering | ⚪ | ✅ | ⚪ | L27 |

## What the comparison says

AIplane's strongest position is the combination of a user-facing AI workspace, a governed application gateway, and a business-agent lifecycle in one self-hosted product. The most distinctive documented areas are browser control in a user's own browser, agent publication to a website or A2A channel, repeatable tests before publication, durable human decisions in an inbox, and version-matched documentation shipped with each installation.

Request priority gives administrators a shared Low/Medium/High setting for chat and API requests from any application. AIplane orders waiting requests by their authenticated priority, retains FIFO within each class and preserves running calls. Its queue belongs to one process; sustained higher-priority traffic can make lower classes time out. Backend forwarding depends on reported scheduling support or an explicit mapping and an enabled upstream scheduler. LiteLLM documents numeric, request-supplied priorities as a beta feature for testing only. Its reviewed scheduler guide does not establish the same group/token administration or backend translation, and the reviewed Open WebUI administration guide does not establish these scheduling capabilities; those entries remain unknown. See A36 and L31 for the operating conditions.

AIplane is behind LiteLLM on documented provider breadth, the standalone Python SDK, the batch API surface, broad third-party logging/observability integrations, Terraform deployment modules, and documented multi-replica scaling. Those are concrete gaps, not wording problems. AIplane's documented deployment topology is one application replica; do not position it as horizontally scalable today.

Open WebUI documents stronger end-user collaboration and chat customization in areas such as shared channels, multi-model side-by-side chats, a centralized file manager, and user-selectable local-password authentication. AIplane's advantage is deeper integration between employee chat, application/API governance, and business-agent lifecycle controls. The products overlap substantially in chat, knowledge retrieval, tools, voice and image workflows; check each row's conditions before calling them equivalent.

Browser control needs a narrow definition. AIplane's documented extension acts in the user's existing, authenticated browser. Open WebUI has adjacent browser and desktop capabilities, but its official Chrome-extension repository is archived and marked deprecated at the revision checked here; it points users to the separate desktop app. The reviewed current product sources do not establish that the desktop app controls the same authenticated browser tab. That is why the matrix marks the exact maintained-extension feature as absent while marking broader browser automation as partial. This is a specific AIplane advantage in the current comparison, not a claim that no Open WebUI community add-on or third-party extension can do similar work.

The marks do not establish comparative performance, security quality, or total cost. Those require deployment-specific tests, threat models and pricing evidence. “No” applies only to the named capability and documented product surface; it does not mean the vendor could not add it or that an extension could not approximate it.

AIplane's GDPR and NDA pool flags are operator declarations. They do not inspect a provider's contracts or certify compliance. LiteLLM OSS already includes virtual keys, user/team controls, budgets, spend tracking, routing, and request/response logging. Its SSO is free for up to five users; SCIM, organization/team-scoped delegated administration, team-member permission customization, model-specific budgets per key, selected built-in moderation integrations, key/team-scoped guardrails, secret managers, automated virtual-key rotation, IP allowlists, team-specific logging controls, and multi-region deployment are documented as Enterprise capabilities. This distinction matters: the matrix does not imply that LiteLLM has no access control in OSS; it separates baseline controls from paid enterprise governance. Open WebUI's feature marks describe the documented self-hosted product and can still depend on configuration or connected services.

## Evidence sources

AIplane sources link to this repository's user and operator documentation. LiteLLM and Open WebUI sources link to their official public documentation. The source keys are deliberately reusable so a maintainer can update a page without repeating long URLs in every table row.

### AIplane

| Key | Source |
|---|---|
| A1 | [Deployment guide](operations/deployment.md) |
| A2 | [Chat guide](guide/chat.md) |
| A3 | [HTTP API reference](reference/api.md) |
| A4 | [Models and routing](admin/models.md), [tool inventory](tools-inventory.md) |
| A5 | [Models and routing](admin/models.md) |
| A6 | [Models and routing](admin/models.md#create-an-automatic-route) |
| A7 | [Fallbacks and temporary outages](admin/models.md#fallbacks-and-temporary-outages) |
| A8 | [Access, tokens and limits](admin/access.md) |
| A9 | [Agent builder](agent-guide/create.md), [agent permissions](agent-guide/permissions.md) |
| A10 | [Knowledge collections](admin/knowledge.md), [tools and integrations](guide/tools-and-integrations.md), [tool inventory](tools-inventory.md) |
| A11 | [Model settings](admin/models.md), [content guard settings](admin/settings.md#access-and-content-guard) |
| A12 | [Approvals and handoffs](guide/automation-and-inbox.md), [agent decisions](agent-guide/run-observe.md) |
| A13 | [Agent runs and activity](agent-guide/run-observe.md) |
| A14 | [Account, memory and usage](guide/account-and-usage.md) |
| A15 | [Tools, integrations and skills](guide/tools-and-integrations.md), [tool inventory](tools-inventory.md) |
| A16 | [Language picker](guide/getting-started.md#adjust-the-interface) |
| A17 | [Tools and integrations](guide/tools-and-integrations.md), [tool inventory](tools-inventory.md) |
| A18 | [Files and canvas](guide/files-and-canvas.md) |
| A19 | [System settings](admin/settings.md#chat-and-documents), [files and documents](guide/files-and-canvas.md), [tool inventory](tools-inventory.md) |
| A20 | [Tools and integrations](guide/tools-and-integrations.md), [tool inventory](tools-inventory.md) |
| A21 | [Tools and integrations](guide/tools-and-integrations.md), [integrations administration](admin/integrations.md) |
| A22 | [System settings](admin/settings.md#tools-and-data-services), [deployment](operations/deployment.md) |
| A23 | [Tool inventory](tools-inventory.md) |
| A24 | [Files and documents](guide/files-and-canvas.md), [tool inventory](tools-inventory.md) |
| A25 | [Test and publish agents](agent-guide/test-publish.md) |
| A26 | [Agent channels and external agents](agent-guide/run-observe.md), [agent test and publication](agent-guide/test-publish.md), [HTTP API reference](reference/api.md) |
| A27 | [Schedules, webhooks and inbox](guide/automation-and-inbox.md), [agent runs and inbox](agent-guide/run-observe.md) |
| A28 | [Operator settings](admin/settings.md), [environment reference](reference/environment.md) |
| A29 | [Agent visitor lifetime, rates and retained data](agent-guide/run-observe.md#visitor-lifetime-rates-and-retained-data) |
| A30 | [Deployment prerequisites and scaling](operations/deployment.md#production-prerequisites) |
| A31 | [Deployment options](operations/deployment.md) |
| A32 | [Backup and recovery](operations/backup-recovery.md) |
| A33 | [Versioned installed documentation](documentation-system.md) |
| A34 | [Monitoring](operations/monitoring.md), [Prometheus metrics settings](admin/settings.md#prometheus-metrics) |
| A35 | [Verify images](operations/deployment.md#verify-images), [what a build publishes](releases.md#what-a-build-publishes) |
| A36 | [Request priority](request-priority.md), [group and token administration](admin/access.md#prioritize-chat-and-api-requests), [backend forwarding](admin/models.md#backend-priority-forwarding) |

### LiteLLM

| Key | Official source |
|---|---|
| L1 | [LiteLLM README](https://github.com/BerriAI/litellm/blob/main/README.md) |
| L2 | [Proxy endpoints](https://docs.litellm.ai/docs/proxy/quick_start) |
| L3 | [Python SDK](https://docs.litellm.ai/docs/) |
| L4 | [Provider integrations](https://docs.litellm.ai/docs/providers) |
| L5 | [Load balancing, routing and fallbacks](https://docs.litellm.ai/docs/routing) |
| L6 | [Traffic mirroring and routing](https://docs.litellm.ai/docs/proxy/traffic_mirroring) |
| L7 | [Authentication and access control](https://docs.litellm.ai/docs/proxy/virtual_keys) |
| L8 | [Teams, users and model access](https://docs.litellm.ai/docs/proxy/team_budgets) |
| L9 | [Budgets and rate limits](https://docs.litellm.ai/docs/proxy/users) |
| L10 | [Spend tracking](https://docs.litellm.ai/docs/proxy/cost_tracking) |
| L11 | [Guardrails](https://docs.litellm.ai/docs/proxy/guardrails/quick_start) |
| L12 | [Memory management](https://docs.litellm.ai/docs/proxy/memory) |
| L13 | [Skills and agent gateway](https://docs.litellm.ai/docs/agent_harness) |
| L15 | [Web search and tools](https://docs.litellm.ai/docs/completion/input) |
| L16 | [MCP gateway](https://docs.litellm.ai/docs/mcp) |
| L18 | [Plugins and callbacks](https://docs.litellm.ai/docs/proxy/logging) |
| L19 | [Agent gateway](https://docs.litellm.ai/docs/agent_harness) |
| L20 | [Scheduled and background tasks](https://docs.litellm.ai/docs/) |
| L21 | [Logging integrations](https://docs.litellm.ai/docs/proxy/logging) |
| L22 | [Metrics and observability](https://docs.litellm.ai/docs/proxy/prometheus) |
| L23 | [Evaluation](https://docs.litellm.ai/docs/proxy/evals) |
| L24 | [Secrets and credential management](https://docs.litellm.ai/docs/proxy/enterprise) |
| L25 | [Production deployment](https://docs.litellm.ai/docs/proxy/deploy) |
| L26 | [Image verification](https://docs.litellm.ai/docs/agent_resources) |
| L27 | [Enterprise plans and support](https://docs.litellm.ai/docs/enterprise) |
| L28 | [Supported API endpoints](https://docs.litellm.ai/docs/supported_endpoints) |
| L29 | [Enterprise features and license requirements](https://docs.litellm.ai/docs/enterprise) |
| L30 | [Role-based access controls, including OSS and Enterprise scopes](https://docs.litellm.ai/docs/proxy/access_control) |
| L31 | [Beta request prioritization](https://docs.litellm.ai/docs/scheduler), reviewed 11 October 2026 |

### Open WebUI

| Key | Official source |
|---|---|
| O1 | [Feature overview](https://docs.openwebui.com/features/) |
| O2 | [Models and agents](https://docs.openwebui.com/features/workspace/models/) |
| O3 | [Extensibility](https://docs.openwebui.com/features/extensibility/plugin/) |
| O4 | [Model comparison and evaluation](https://docs.openwebui.com/features/administration/) |
| O5 | [Authentication and access](https://docs.openwebui.com/features/authentication-access/) |
| O6 | [Administration and analytics](https://docs.openwebui.com/features/administration/) |
| O7 | [Tools and approval controls](https://docs.openwebui.com/features/extensibility/plugin/tools/) |
| O8 | [Chat features](https://docs.openwebui.com/features/chat-conversations/chat-features/) |
| O9 | [Channels](https://docs.openwebui.com/features/channels/) |
| O10 | [Data controls](https://docs.openwebui.com/features/chat-conversations/data-controls/) |
| O12 | [File and document handling](https://docs.openwebui.com/features/chat-conversations/data-controls/files/) |
| O13 | [Knowledge and RAG](https://docs.openwebui.com/features/workspace/knowledge/) |
| O14 | [Central file manager](https://docs.openwebui.com/features/chat-conversations/data-controls/files/) |
| O15 | [Open Terminal](https://docs.openwebui.com/features/open-terminal/) |
| O16 | [Automations](https://docs.openwebui.com/features/chat-conversations/chat-features/automations/) |
| O17 | [Webhooks and integrations](https://docs.openwebui.com/features/administration/) |
| O18 | [Deployment and observability](https://docs.openwebui.com/features/) |
| O19 | [Security and data controls](https://docs.openwebui.com/features/chat-conversations/data-controls/) |
| O20 | [Deployment and scaling](https://docs.openwebui.com/features/) |
| O22 | [Official Chrome extension repository (archived and deprecated)](https://github.com/open-webui/extension) |
| O23 | [Open WebUI Desktop](https://github.com/open-webui/desktop) |
| O24 | [Open WebUI API endpoints](https://docs.openwebui.com/reference/api-endpoints/) |

## Git revisions compared

The product and documentation sources were checked against these exact repository revisions. The public documentation baseline was reviewed on **4 October 2026**; the request-priority update uses the **11 October 2026** documentation check described above. The revision pins identify the implementation baselines even as upstream `main` branches move.

| Product/source | Repository | Revision |
|---|---|---|
| croit AIplane (implementation baseline reviewed) | [`croit/aiplane`](https://github.com/croit/aiplane) | [`e8285ffdd457ef9b5d6899e438c3484a7b8f36cf`](https://github.com/croit/aiplane/commit/e8285ffdd457ef9b5d6899e438c3484a7b8f36cf) |
| croit AIplane (request-priority implementation reviewed) | [`croit/aiplane`](https://github.com/croit/aiplane) | [`d5b81c2abcdb77f96522c4f10b9cecf0c691c16d`](https://github.com/croit/aiplane/commit/d5b81c2abcdb77f96522c4f10b9cecf0c691c16d) |
| LiteLLM (implementation and documentation) | [`BerriAI/litellm`](https://github.com/BerriAI/litellm) | [`e1d16f51d14849c1b3decf17cd81a3bcb4863dca`](https://github.com/BerriAI/litellm/commit/e1d16f51d14849c1b3decf17cd81a3bcb4863dca) |
| Open WebUI (implementation) | [`open-webui/open-webui`](https://github.com/open-webui/open-webui) | [`8bd8b4fac5e059578ac0c74b3c18d11139f88b7d`](https://github.com/open-webui/open-webui/commit/8bd8b4fac5e059578ac0c74b3c18d11139f88b7d) |
| Open WebUI (public feature documentation) | [`open-webui/docs`](https://github.com/open-webui/docs) | [`e6e9151cae4d4aec79d1a2169475c0a41f47cad9`](https://github.com/open-webui/docs/commit/e6e9151cae4d4aec79d1a2169475c0a41f47cad9) |
| Open WebUI (archived official Chrome extension) | [`open-webui/extension`](https://github.com/open-webui/extension) | [`635b90c685649806b2b1e06a39c6e3e2479195d5`](https://github.com/open-webui/extension/commit/635b90c685649806b2b1e06a39c6e3e2479195d5) |
| Open WebUI Desktop (replacement project referenced by the extension repository) | [`open-webui/desktop`](https://github.com/open-webui/desktop) | [`0931b46d66e06bfb7d0b6a4aa065e6db338a1000`](https://github.com/open-webui/desktop/commit/0931b46d66e06bfb7d0b6a4aa065e6db338a1000) |
