# STATUS: llm-generated, unreviewed — pending native-speaker QA

pools-page-title = 上游池 — LLM Gateway
pools-heading = 上游池
pools-description = 按类型和选择器策略将后端分组为池。更改会保存到数据库，但只有在您点击“应用更改”后才会生效。

pools-fallbacks-heading = 未知模型回退
pools-fallbacks-description = 当请求指定了网关从未见过的模型时，为该类型替换使用此模型。留空 = 未命中时返回 404。

pools-add-heading = 添加池
pools-field-name = 名称
pools-field-kind = 类型
pools-field-strategy = 策略
pools-field-fallback-offline = 离线回退模型
pools-field-fallback-offline-placeholder = 当所有后端都离线时提供服务
pools-field-models = 提供的模型（白名单，逗号分隔）
pools-field-models-hint = 设置后，对于启用 /models 探测的后端仅提供这些 id，其余以划线显示。留空 = 提供后端报告的所有模型。
pools-field-voices = 语音（每行 lang=voice）
pools-field-offer-voices = 可选语音（每行一个，供用户选择）
pools-field-backends = 后端
pools-no-backends = 尚未定义任何后端。请先在“后端”页面添加一个。
pools-field-gdpr = 符合 GDPR
pools-field-nda = 受 NDA 保护
pools-field-enforce-limits = 强制执行速率限制与配额
pools-save-pool = 保存池
pools-add-pool = 添加池
pools-delete-pool = 删除

pools-error-name-required = 池名称为必填项
pools-error-invalid-kind = 无效的池类型 `{ $kind }`
pools-saved = 已保存池 `{ $name }` — 点击“应用更改”以重新加载
pools-deleted = 已删除池 `{ $name }` — 点击“应用更改”以重新加载
pools-fallback-saved = { $kind } 回退已设置为 `{ $model }`
pools-fallback-cleared = { $kind } 回退已清除

pools-field-allowed-groups = åè®¸çç»
pools-field-allowed-groups-hint = åè®¸æ¥çåä½¿ç¨æ­¤æ± æ¨¡åçç½å³ç»ï¼ç¨éå·åéï¼ãçç©º = ææäººãç®¡çåå§ç»ææéãå¨ ç®¡ç â ç» ä¸­ç®¡çç»ã

# Duplicate-name guard on the Add-pool form.
pools-error-name-exists = 名为 `{ $name }` 的池已存在 — 再次点击“添加池”以覆盖它，或更改名称
pools-overwrite-hint = 该名称已存在。再次保存将覆盖现有的池——包括其后端、模型、语音和合规标记。请更改名称以创建独立的池。
# Why the strategy choice matters for self-hosted replicas.
pools-field-strategy-hint = prefix_affinity 让同一会话继续落在已持有其 KV 缓存的副本上（多 GPU 的聊天/智能体流量首选——其他策略会把相邻回合分散到不同副本，每次都要重付一次完整 prefill），同时在某个后端确实更繁忙时仍会分流。least_inflight 按当前负载均衡；round_robin 按权重轮转。
# What the pool advertises, and how many replicas serve each name (U6/U7).
upstreams-coverage-heading = 客户端看到的内容
upstreams-coverage-hint = 正是 GET /v1/models 为该池返回的名称，并附上当前有多少后端能够提供它。任何低于“全部”的情况都意味着该名称让你的一部分硬件在空转。
upstreams-coverage-full-title = 该池的每个后端都提供这个名称。
upstreams-coverage-partial-title = 只有部分后端提供这个名称——相关请求只用到池的一部分，其余在空转。通常是别名的目标与某个后端公布的模型不一致。
upstreams-coverage-none-title = 当前没有后端能提供这个名称。相关请求会收到临时不可用错误。

# The per-pool problem summary (U8).
upstreams-problems-heading = 该池未完全正常工作
upstreams-problem-no-backends = 未分配后端 —— 该池中没有任何东西可以处理请求。
upstreams-problem-all-drained = 所有后端都已排空以进行维护，不会有请求路由到这里。
upstreams-problem-all-down = 没有可用后端：请求会等待其中一个恢复，随后收到临时不可用错误。
upstreams-problem-auth = 凭据被拒绝的后端：{ $backends }。它们的模型发现已关闭，不会公布任何模型。
upstreams-problem-no-models = 未公布任何模型的后端：{ $backends }。不会有请求路由到它们，裸别名在那里也无从绑定。
upstreams-problem-broken-aliases = 无法路由的别名：{ $aliases }。每个都指向其后端不提供的模型，或没有可绑定的目标。
upstreams-problem-partial-coverage = 仅由部分池提供：{ $models }。这些名称的请求用到的副本少于你实际拥有的数量。
upstreams-problem-unserved-allowlist = 在“模型”中列出但无人提供：{ $models }。
upstreams-problem-missing-backends = 已分配但不再存在的后端：{ $backends }。
# Live name-clash note on the add form (U9).
pools-name-taken = 同名的池已存在——保存会替换它，包括其后端和模型。请换一个名称以创建新池。
# The apply diff (U10).
upstreams-apply-diff-summary = 查看应用后会发生哪些变化
upstreams-diff-pool-added = 新池 { $pool } 开始提供服务
upstreams-diff-pool-removed = 池 { $pool } 停止提供服务
upstreams-diff-pool-kind = 池 { $pool }：类型 { $from } → { $to }
upstreams-diff-pool-strategy = 池 { $pool }：策略 { $from } → { $to }
upstreams-diff-backend-joins = { $backend } 加入池 { $pool } 并开始接收流量
upstreams-diff-backend-leaves = { $backend } 离开池 { $pool } 并停止接收流量
upstreams-diff-backend-url = { $backend }：基础 URL { $from } → { $to }（其已发现的模型将重新探测）
upstreams-diff-backend-limits = { $backend }：权重 { $weight }，最大并发 { $inflight }
upstreams-diff-backend-health-path = { $backend }：健康检查路径 → { $to }
