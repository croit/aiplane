# STATUS: llm-generated, unreviewed — pending native-speaker QA


pools-field-name = 名称
pools-field-kind = 类型
pools-field-models = 提供的模型（逗号分隔）
pools-field-backends = 后端
pools-save-pool = 保存池
pools-delete-pool = 删除
pools-delete-confirm = 删除资源池“{ $name }”？其中的后端会保留，并变为未分配。

# SPA 池卡片：两行摘要与删除确认。
pools-add-heading = 添加池
pools-fallbacks-heading = 未知模型回退
pools-fallbacks-description = 当请求指定了网关从未见过的模型时，为该类型替换使用此模型。留空 = 未命中时返回 404。
upstreams-problems-heading = 该池未完全正常工作
pools-name-taken = 同名的池已存在——保存会替换它，包括其后端和模型。请换一个名称以创建新池。
pools-field-strategy = 策略
pools-field-strategy-hint = prefix_affinity 让同一会话继续落在已持有其 KV 缓存的副本上（多 GPU 的聊天/智能体流量首选——其他策略会把相邻回合分散到不同副本，每次都要重付一次完整 prefill），同时在某个后端确实更繁忙时仍会分流。least_inflight 按当前负载均衡；round_robin 按权重轮转。
pools-field-fallback-offline = 离线回退模型
pools-field-fallback-offline-placeholder = 当所有后端都离线时提供服务
pools-field-models-hint = 设置后，只提供这些 id，且不再询问后端有哪些模型 — 适用于 /models 列出的是别的内容或遗漏了某个模型的情况。留空 = 通过每个后端的 /models 发现模型。
pools-field-models-chips = 由此资源池的后端上报 — 点击可固定或取消
pools-field-allowed-groups = 允许的组
pools-field-allowed-groups-hint = 允许查看和使用此池模型的 AIplane 组。未选择 = 所有人。管理员始终有权限。在 管理 → 组 中管理组。
pools-field-voices = 语音（每行 lang=voice）
pools-field-offer-voices = 可选语音（每行一个，供用户选择）
pools-no-backends = 尚未定义任何后端。请先在“后端”页面添加一个。
pools-field-enforce-limits = 强制执行速率限制与配额

upstreams-problem-no-backends = 未分配后端 —— 该池中没有任何东西可以处理请求。
upstreams-problem-all-drained = 所有后端都已排空以进行维护，不会有请求路由到这里。
upstreams-problem-all-down = 没有可用后端：请求会等待其中一个恢复，随后收到临时不可用错误。
upstreams-problem-auth = 凭据被拒绝的后端：{ $backends }。它们的模型发现已关闭，不会公布任何模型。
upstreams-problem-no-models = 未公布任何模型的后端：{ $backends }。不会有请求路由到它们，裸别名在那里也无从绑定。
upstreams-problem-broken-aliases = 无法路由的别名：{ $aliases }。每个都指向其后端不提供的模型，或没有可绑定的目标。
upstreams-problem-partial-coverage = 仅由部分池提供：{ $models }。这些名称的请求用到的副本少于你实际拥有的数量。
upstreams-problem-unserved-allowlist = 在“模型”中列出但无人提供：{ $models }。
upstreams-problem-missing-backends = 已分配但不再存在的后端：{ $backends }。
