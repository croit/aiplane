# STATUS: llm-generated, unreviewed — pending native-speaker QA
# The upstream board and the backend editor: the pool band's status word, the
# editor's fields, and the connection-test outcomes.
backends-status-up = 正常


# Backend CRUD editor (add/edit/delete backends stored in the DB topology).
backends-field-name = 名称
backends-field-base-url = 基础 URL
backends-field-pool = 池
backends-field-pool-none = （无）
backends-field-pools = 资源池
backends-field-pools-hint = 账号提供的每种模型类型对应一个资源池。该类型可加入一个现有的空池、在同一次保存中创建新池，或不提供服务。已包含其他后端的池不会被提供 — 混合会将该后端固定到此允许列表。
backends-pool-not-served = 不提供
backends-pool-new = 新建：openrouter-{ $kind }
backends-save-backend = 保存后端
backends-delete-backend = 删除
backends-delete-confirm = 删除后端“{ $name }”？其资源池会保留其余后端。
backends-advanced = 高级设置

backends-field-api-key = API 密钥
backends-field-api-key-keep = 留空以保留当前密钥
backends-field-api-key-stored = 已存储密钥：{ $hint }
backends-field-api-key-clear = 删除已保存的密钥

# An alias that is configured but would not route.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.

# SPA 后端行：分流开关、每小时负载与删除确认。
backends-add-heading = 添加后端
backends-name-taken = 同名的后端已存在——保存会覆盖它，包括基础 URL、密钥、模型和所属池。请换一个名称以添加第二个后端。
backends-field-api-key-placeholder = API 密钥（加密存储）
backends-field-api-key-env = API 密钥环境变量
backends-field-health-path = 健康检查路径
backends-field-pool-hint = 将此后端分配到一个池。位于多个池中的后端会被收敛到此处所选的池。
backends-field-weight = 权重
backends-field-max-inflight = 最大处理中数
backends-field-models = 模型（逗号分隔；留空 = 通过 /models 发现）
backends-field-aliases = 别名（每行 name=target）
backends-field-extra-headers = 额外请求头（每行 name: value）
backends-field-extra-headers-hint = 附加到发往该后端的每个请求，在 API 密钥之后。例如 Anthropic 要求的 anthropic-version: 2023-06-01。
backends-field-supports-edit = 支持图像编辑
backends-no-models-title = 该后端未公布任何模型，因此不会有请求路由到它，裸别名也无从绑定。通常是探测始终未返回数据。
backends-no-models = 无模型
backends-key-env-badge = 密钥：环境变量 { $var }
backends-key-env-unset-badge = 环境变量 { $var } 未设置
backends-enabled-hint = 关闭以将该后端排空进行维护。立即生效。其模型仍为已知，因此池中其他后端会接管，客户端只会看到临时不可用，而不是“找不到模型”。
backends-enabled-label = 接收流量（{ $name }）
backends-activity-summary = 15分钟 { $m15 } · 30分钟 { $m30 } · 60分钟 { $m60 }
backends-alias-target-title = 别名 → { $target }
backends-alias-missing-title = 别名 → { $target } — 此后端未上报该模型
backends-alias-disabled-title = 裸别名已禁用 — 此后端提供多个模型，请为其指定明确的目标（映射表单）
backends-alias-disabled-label = { $name }（已禁用）
backends-fallback-offline-title = fallback_offline：当此池中某已知模型的所有后端都离线时使用
backends-fallback-offline-badge = 离线 ↩ { $model }
backends-pool-empty = 此池中没有后端。

backends-test-button = 测试连接
backends-test-hint = 使用上面填写的凭据调用该 URL。不会保存任何内容。
backends-test-insert-hint = 上游公布的模型 id —— 点击即可补全光标所在的别名行：
backends-test-ok = 可达，认证通过（{ $source }），发现 { $count } 个模型。
backends-test-ok-no-models = 可达且认证通过（{ $source }），但响应既不是 OpenAI 也不是 TypeSafe 的 /models 列表，模型发现无法解析。该后端只能提供你在“模型”中列出的 id。
backends-test-auth-failed = 被拒绝，HTTP { $status }：凭据未被接受（{ $source }）。在修好之前模型发现保持关闭，后端不会公布任何模型。
backends-test-http-error = { $url } 返回 HTTP { $status }。
backends-test-upstream-said = 上游响应：{ $detail }
backends-test-unreachable = 无法连接 { $url }：{ $err }
backends-test-timeout = { $url } 在 { $secs } 秒内未响应。
backends-test-key-typed = 使用上面输入的密钥
backends-test-key-stored = 使用已保存的密钥
backends-test-key-env = 来自环境变量 { $var }
backends-test-key-env-unset = 环境变量 { $var } 未设置 —— 请求未携带任何凭据
backends-test-key-none = 未发送凭据
backends-error-base-url-required = 基础 URL 为必填项

backends-parallel-mismatch = 服务器同时处理 { $parallel } 个
backends-detect-profile = 已识别：{ $profile }

# OpenRouter catalog: one backend, one key, the account's models, and a
# purchase-price ceiling per selected model.
openrouter-heading = OpenRouter
openrouter-description = 一个 OpenRouter 连接。选择此网关可使用的模型及每个模型的最高价格；没有提供商低于其上限的模型不会提供。
openrouter-connected = 已连接
openrouter-key-label = 密钥
openrouter-credits = 额度
openrouter-refresh = 刷新价格
openrouter-backend = 后端
openrouter-search = 搜索模型
openrouter-select-all = 全选
openrouter-deselect-all = 取消全选
openrouter-selected-count = 已选 { $count } 个
openrouter-model = 模型
openrouter-price = 标价
openrouter-context = 上下文
openrouter-ceiling-prompt = 输入上限 $/1M
openrouter-ceiling-completion = 输出上限 $/1M
openrouter-strategy = 提供商策略
openrouter-strategy-cheapest = 最便宜
openrouter-strategy-fastest = 最快
openrouter-strategy-balanced = 均衡
openrouter-min-throughput = 最低 tokens/s
openrouter-over-ceiling = 超出上限
openrouter-save = 保存选择
openrouter-saved = 选择已保存。
openrouter-no-models = 没有模型匹配此筛选条件。
openrouter-error = OpenRouter: { $message }
openrouter-select-model-first = 请至少选择一个模型。
openrouter-endpoints = 提供商
openrouter-provider-input = 输入 $/1M
openrouter-provider-output = 输出 $/1M
openrouter-provider-cache-read = 缓存读取
openrouter-provider-throughput = tok/s
openrouter-provider-latency = 延迟
openrouter-provider-uptime = 可用性
openrouter-provider-limit = 你的上限
openrouter-ceiling-missing = 未设置上限
openrouter-usage = 用量
openrouter-aria-input-ceiling = { $model } 输入上限
openrouter-aria-output-ceiling = { $model } 输出上限
openrouter-aria-strategy = { $model } 提供商策略
openrouter-aria-min-throughput = { $model } 最低吞吐量
openrouter-cheapest-provider = 最低价
openrouter-provider-fits = 符合
openrouter-provider-excluded = 超出
openrouter-providers-fit = 你的上限内有 { $fit } / { $total } 家提供商
openrouter-providers-hidden = 因上限隐藏了 { $count } 个提供商
openrouter-providers-none-fit = 没有提供商符合你的上限 — 提高 Max input 或 Max output。
openrouter-providers-fit-short = { $fit } / { $total } 符合
openrouter-price-unknown = 价格未检查
openrouter-price-unknown-hint = 此模型的提供商价格尚未读取——展开该行即可查看。
openrouter-cheapest = 最低价：{ $provider }，{ $prompt } / { $completion } 每 1M
openrouter-use-cheapest = 采用最低价
openrouter-compare = 在 OpenRouter 上比较
openrouter-provider-error = 无法读取提供商：{ $message }
openrouter-provider-none = OpenRouter 未报告此模型的提供商。
openrouter-provider = 提供商
openrouter-add-heading = 添加 OpenRouter
openrouter-add-description = 一个密钥，全部配置完成：后端加上每个模型类型的资源池（chat、transcription、speech、system_one、embedding、image、rerank），命名为 openrouter-<类型>。已存在同名的池将被加入。模型选择与价格上限在 OpenRouter 面板中完成。
openrouter-add-save = 添加 OpenRouter
openrouter-kinds = 模型类型
openrouter-kind-selected-count = 已选 { $count } 个
openrouter-kind-catalog-count = 目录 { $count } 个
openrouter-kind-not-enabled = 未启用
openrouter-key-change = 更换密钥
openrouter-key-placeholder = 新的 API 密钥
openrouter-key-save = 保存密钥
openrouter-key-saved = 密钥已更新
openrouter-sort = 排序
openrouter-save-changed = 保存选择 · { $count } 项已更改
openrouter-dirty-confirm = { $kind} 有未保存的更改。放弃？
openrouter-provider-field = 提供商
openrouter-provider-openai = OpenAI 兼容
openrouter-provider-openrouter = OpenRouter
openrouter-provider-hint = OpenRouter 读取账户目录，并在每次请求时执行每个模型的价格上限。其他提供商是普通的 OpenAI 兼容 HTTP。
openrouter-ceiling-required = 每个已选模型都需要两个价格上限。
openrouter-max-price = 最高 $/1M
openrouter-voices = 声音
