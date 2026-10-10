# Request priority

AIplane uses low, medium and high request priority for chat and API model calls.
Administrators configure a system default and priorities on existing gateway
groups. The highest priority of a person's resolved groups applies; with no
explicit group priority the system default applies. A token can inherit its owner's priority or
override it, including lowering it for background work.

Strict priority dispatches executable high requests before medium and low.
Within a class requests use arrival order. A request whose model or access
cannot use a free backend does not block other executable requests. Running
calls keep their slots until completion. Lower classes can time out under
sustained higher-priority traffic. Queue deadlines and cancellation release
waiting requests and any slot assigned to them.

Capacity retries share one priority-ordered dispatch pass per 250 ms polling
interval, so each waiting request does not rescan the entire queue. A zero
waiting budget still permits one admission attempt. Stopping a chat cancels
its pending model or automatic-selector admission without waiting for GPU
capacity; cancellation is checked every 50 ms while admission is pending.

## Shared mechanisms

- The DB-backed gateway groups and RBAC resolver determine group membership.
- The existing tokens table stores the token override; token management remains
  administrator-only for priority changes.
- The typed operator settings catalog stores the switch and system default.
- `UpstreamRegistry`, `PoolAccess`, aliases, fallback routing and prefix affinity
  continue to select authorized models and backends.
- `route_or_wait` owns waiting, with one registry-owned priority queue, and
  `Acquired` owns each running slot.
- Backend identification supplies reported scheduling capabilities. The existing
  backend editor stores automatic, gateway-only or manual parameter mappings.
- One priority mapping translates the effective class at the final dispatch
  boundary, including tool rounds, retries and fallback backends.
- Fluent catalogs supply all labels in the existing admin forms.

## Administration

![Token priorities in the administration screen, using synthetic demo data.](img/guide/request-priority.png)

Enable **request prioritization** under **Administration → Settings → Access → Gateway** (`gateway.priority_enabled`, disabled by default) and choose the system default. Edit a
group's priority under Administration → Access → Groups. Under Tokens, choose
inherit, low, medium or high. For the ERP integration choose high on its token;
other tokens and browser chat continue to inherit their owner's groups.

Token save failures restore the previous selection and clear earlier success
messages. Backend save errors receive keyboard focus and scroll into view;
priority errors use the administrator's selected UI language.

Backend priority defaults to automatic. Automatic forwarding requires reported
enabled scheduling and a known priority direction. Gateway-only mode sends no
backend priority. Manual mode explicitly maps the three classes to a top-level
integer parameter; it overrides detection. Parameter names cannot replace model,
messages, tools, streaming or generation controls. Client-supplied priority is
replaced or removed; it cannot elevate the authenticated caller.

vLLM requires `--scheduling-policy priority`; lower integers run first. SGLang
requires `--enable-priority-scheduling` with `--schedule-policy fcfs` or `lof`; larger integers run first unless
`--schedule-low-priority-values-first` is set. Identifying the software alone
does not prove these flags are enabled. Other backends still use gateway
priority, and receive a parameter only when configured explicitly.

The gateway queue and slot counters belong to one AIplane process. Multiple
instances do not share this admission queue. Backend scheduling applies to
requests received by that backend across callers when its scheduler is enabled.
Forwarding priority does not guarantee a latency bound or reserve GPU capacity.

## API and configuration

`PUT /api/v0/admin/groups` accepts `priority: "low" | "medium" | "high" | null`.
`PUT /api/v0/admin/tokens/{id}/priority` accepts `{"priority":"high"}`;
`null` restores inheritance. Both require an administrator session. Token owners
cannot raise their priority through personal token endpoints or request bodies.

`PUT /api/v0/admin/backends` accepts `priority` alongside the existing backend
fields. Save the complete editor and apply topology changes. Modes:

```json
{"mode":"auto"}
{"mode":"gateway"}
{"mode":"custom","mapping":{"field":"priority","low":1,"medium":0,"high":-1}}
```

The last example is the vLLM mapping. Its scheduler flag must be configured in
its container command; AIplane does not change model-server startup arguments.
SGLang automatic mode requires `/server_info` to report the enabled flag,
`fcfs` or `lof`, and `schedule_low_priority_values_first`. Missing fields leave
forwarding disabled. vLLM, Ollama, llama.cpp and generic endpoints need manual
mapping for forwarding; they always use the gateway queue when prioritization
is enabled. Gateway-only works without any model-server priority support.

The gateway settings are saved through `POST /api/v0/admin/settings`. Existing
`gateway.upstream_wait_secs` bounds slot waiting (120 seconds by default).
Set each backend's maximum in-flight requests to suit its actual serving
capacity: a gateway cannot reorder requests already admitted to a server that
ignores priority. A running low-priority request is not interrupted by AIplane.
Prefix affinity selects replicas within the scheduling decision, preserving
prompt-cache locality where capacity permits. Priority grants neither model
access nor additional usage allowance.

Backend references: [vLLM request protocol](https://docs.vllm.ai/en/latest/api/vllm/entrypoints/openai/chat_completion/protocol/) and [SGLang scheduling policy](https://github.com/sgl-project/sglang/blob/main/python/sglang/srt/managers/schedule_policy.py).
