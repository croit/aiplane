# Agent spec field reference

The agent's advanced JSON editor edits one typed configuration. Save validates the draft; Publish applies stricter checks for complete references, grants and production requirements. Unknown keys are rejected. Correct the reported path rather than assuming an unsupported option is ignored.

This reference describes the field structures. Use [Create an agent](create.md), [Permissions](permissions.md), [Test and publish](test-publish.md) and [Run and observe](run-observe.md) for workflows. Resource IDs in a spec must come from the installation's actual catalogs; a structurally valid example cannot establish that a resource exists.

## Profile, scope and main

| Path | Purpose |
|---|---|
| `profile.display` | Display name |
| `profile.avatar` | Profile avatar value |
| `profile.color` | Widget color, `#rrggbb` |
| `scope.topics` | Topics the agent handles |
| `scope.refusal` | Refusal wording for out-of-scope requests |
| `scope.strict` | Enable the topic guard before the main run |
| `scope.classifier_model` | Guard model; main model when unset |
| `main.model` | Chat model; gateway default when unset |
| `main.instructions.orchestration` | How the agent works and uses its abilities |
| `main.instructions.response` | How it writes its answer |
| `main.tools` | Selected tool IDs |
| `main.skills` | Selected skill names |
| `main.budget.rounds` | Model/tool-round ceiling |
| `main.budget.seconds` | Elapsed run budget |
| `main.budget.tokens` | Token budget |

The main run's omitted rounds use the default effort's (`low`) round budget. Omitted seconds/tokens do not introduce a ceiling at this spec field. Operator resource limits still apply.

### Tool resource settings

`main.tool_resources.<tool>` accepts:

| Field | Purpose |
|---|---|
| `bind` | Argument-name → gateway-supplied source |
| `permission` | `always_allow` or `always_ask`; unset uses default policy |
| `approval_timeout` | Approval waiting duration; default one hour |

Bind sources are `"state.<slot-or-path>"`, `"route.<name>"` or `{"const": <JSON value>}`. The gateway removes bound parameters from the model-visible schema and overwrites any supplied value. `route.<name>` requires a dispatching route to provide that name.

Do not bind unverified subject data merely because its value looks plausible. Publish validation checks whether routing gates guarantee trusted provenance for state-sourced subject bindings.

## State slots

`state` maps slot names to definitions.

| Field | Purpose / applicable type |
|---|---|
| `type` | `string`, `email`, `enum`, `integer`, `number`, `boolean`, `subject` |
| `set_by` | Allowed writers, such as `llm`, `host` or `verifier:<id>` |
| `description` | Meaning of the slot |
| `values` | Allowed enum values |
| `min_length` | Minimum string length |
| `max_length` | Maximum string/email length |
| `minimum`, `maximum` | Integer/number bounds |
| `pattern` | String pattern |
| `schema` | Structured subject schema |
| `order` | Display order of details in the setup assistant |

Constraints on types where they cannot be enforced are errors. `order` is editor metadata; it does not control runtime route evaluation. Provenance and age belong to actual state writes, not claims that a visitor types into a message.

## Verifiers

`verifiers.<id>.kind` chooses `mcp_code`, `lookup` or `host_jwt`. `assurance` describes the verification's actual assurance; choose it for the real mechanism rather than implying stronger identity proof.

### MCP code

| Field | Purpose |
|---|---|
| `assurance` | Declared assurance |
| `connector` | MCP connector holding the code operations |
| `send_tool`, `check_tool` | Tool names; defaults `send_code`, `check_code` |
| `input` | Input selected for verification |
| `email_slot` | Email state slot used by the code flow |
| `writes` | Slot-name → trusted result/input source |
| `max_attempts` | Wrong guesses allowed; default 5, cap 10 |
| `code_ttl` | Code lifetime; default 10 minutes, cap one hour |
| `send_limits.email`, `.ip`, `.session` | Rate limits for sending codes, each `{max, per}` |

The send/check operations must exist on the configured connector and be available to the principal. Rate limits are independent of the broader visitor-message rate policy.

### Lookup

| Field | Purpose |
|---|---|
| `assurance` | Declared assurance |
| `tool` | Lookup tool |
| `inputs` | Tool-input-name → state slot or `{"const": value}` |
| `writes` | Slot-name → trusted result/input source |
| `max_attempts` | Wrong attempts allowed; default 5, cap 10 |

Lookup inputs use `"state.<slot>"` or a constant. Verifier output sources use `result`, `result.<field>` or `input.<argument>`. Check the tool's real return shape before declaring writes.

### Host JWT

| Field | Purpose |
|---|---|
| `assurance` | Declared assurance |
| `algorithm` | Supported JWT algorithm selected by the validator |
| `secret` | Shared verification secret, sealed during save |
| `secret_sealed` | Server-managed sealed representation; preserve when editing existing JSON |
| `public_key` | Public-key verification material |
| `jwks_url` | Key-set endpoint when using remote keys |
| `issuer`, `audience` | Expected token issuer and audience |
| `max_lifetime` | Maximum accepted token lifetime; default 10 minutes, cap 24 hours |
| `claims` | Slot mappings from token claims; a claim name or object of field → claim |

JWT identity is supplied by the host integration, not manufactured by the conversation model. Configure signing/key material and claims consistently with that host. JWKS URLs undergo the outbound-network policy.

## Router, conditions and targets

`router.kind` is `rules` or `classifier`. `router.model` selects its classifier model; `router.order` names route evaluation order. `routes.<name>` contains `description`, `when`, optional `task` and `bind`, and exactly one target (`agent`, `human`, `a2a` or `loop`).

Conditions combine `{"all": [conditions]}`, `{"any": [conditions]}` or `{"not": condition}`. All/any require nonempty arrays and each combinator is the only key of its object. A leaf uses:

| Field | Check |
|---|---|
| `slot` | Declared state slot |
| `set` | Presence/absence |
| `eq` | Equality with a JSON value |
| `in` | Membership in an array of JSON values |
| `provenance` | Required writer: `llm`, `host` or `verifier:<id>` |
| `max_age` | Maximum age of the state write |

Checks in one leaf are ANDed. A leaf must perform at least one check. Missing/invalid state fails checks except an explicit absence check. Gate failures report unmet conditions without exposing state values the model did not write.

Task text can use `{slot}` and `{slot.field}` placeholders, resolved from the main agent state. A dispatched sub-agent receives its task and bound inputs rather than the entire parent conversation. Missing placeholders produce errors identifying what is missing.

### Sub-agent target

`agent` names a published agent. `task` describes its work. `bind` passes resolved arguments down to matching sub-agent tool parameters. Publication checks the reachable agent graph, resource references and trusted-bind requirements; inspect path-specific failures.

### Human target

`human` accepts `notify` (`push`, `slack`, `discord`), `inbox`, `timeout` and `transcript`. Timeout defaults to 30 minutes; transcript inclusion defaults off. A handoff suspends the turn for an authorized human answer. Notification channels do not grant answer rights.

### Remote A2A target

`a2a` accepts `card_url`, `auth`, `finish` and `budget.seconds`. The timeout defaults to 120 seconds and is capped at 900 seconds. `finish.schema` is required at publication for the expected remote result.

Authentication kinds are `bearer`, `api_key` and `oauth_client_credentials`. Fields include `scheme`, `token`, `token_sealed`, `client_id`, `client_secret`, `client_secret_sealed` and `scopes`, according to the chosen kind. Plaintext credentials are sealed during save; the sealed fields are server-managed representations. The card and its advertised endpoint are subject to outbound-network policy.

### Worker/critic loop target

`loop` accepts `worker`, `critic`, `max_iterations`, `budget.seconds` and `budget.tokens`. Worker/critic are agent IDs. Iterations default to 3 and are capped at 10. Set finite budgets suited to the real task, and test both completion and exhaustion.

## Finish schema

`finish.schema` supports these validating keywords: `type`, `properties`, `required`, `enum`, `items` and boolean `additionalProperties`. Types can be a name or list of names. `items` is a single schema for array members. Annotation keywords are `title`, `description`, `default`, `examples` and `$schema`.

Unsupported validating keywords, such as `oneOf` or string `pattern`, are rejected. Do not copy arbitrary JSON Schema expecting the whole standard to be implemented. Slot constraints such as `pattern` are separate from the finish schema subset.

## Publish policy

| Field | Purpose |
|---|---|
| `origins` | Optional additional allowed embed origins |
| `idle_ttl` | Idle visitor-token lifetime; default 30 minutes |
| `retention_days` | Idle conversation retention; default 30 days |
| `audit_retention_days` | Activity-chain retention; default 365 days and at least conversation retention |
| `rate_limits.visitor`, `.ip` | `{max, per}` for each inbound rate scope |
| `budget.monthly_cost`, `.monthly_tokens` | Owner monthly ceilings; unset adds no owner ceiling |
| `output_filter.patterns` | Named regex patterns for provenance-sensitive identifiers |
| `output_filter.action` | `withhold` or `redact`; default withhold |
| `require_passing_tests` | Require a current green draft suite |
| `voice.input`, `.output` | Enable widget voice input/output |
| `voice.transcription_model`, `.speech_model` | Models for enabled directions; gateway defaults when unset |
| `voice.voice` | Offered speech voice; language/model default when unset |
| `a2a.enabled` | Serve A2A; disabled when unset |
| `a2a.skills` | Agent-card skill declarations |

Each A2A advertised skill has `id`, `name`, `description`, optional `tags` and `examples`. These describe the actual published agent; they are distinct from installed instruction bundles selected by `main.skills`.

Durations use values such as `30s`, `15m`, `2h` or `30d`. A rate has positive `max` and a duration `per`. Retention values are positive days. Publication and runtime enforce their corresponding limits; use a test case to verify the intended policy.
