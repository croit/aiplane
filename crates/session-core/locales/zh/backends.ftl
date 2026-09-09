# STATUS: llm-generated, unreviewed — pending native-speaker QA

backends-page-title = 上游后端 — LLM Gateway
backends-heading = 上游后端
backends-description-prefix = 已配置上游池的实时视图——各后端的健康状态、相对于其上限的当前负载，以及每个后端目前提供的模型。仅供查看：路由完全取决于后端通过其
backends-description-suffix = 探测接口报告的信息。
backends-summary = 共 { $total } 个后端 · { $healthy } 个健康 · { $down } 个离线
backends-unknown-fallback-prefix = 未知模型回退 —
backends-empty-prefix = 未配置任何上游池。请在 gateway.toml 中添加
backends-empty-suffix = 块并重启。

backends-fallback-offline-title = fallback_offline：当此池中某已知模型的所有后端都离线时使用
backends-fallback-offline-badge = 离线 ↩ { $model }
backends-pool-empty = 此池中没有后端。

backends-status-down = 离线
backends-status-saturated = 已饱和
backends-status-up = 正常

backends-inflight-label = 处理中 { $load }
backends-activity-summary = 15分钟 { $m15 } · 30分钟 { $m30 } · 60分钟 { $m60 }
backends-no-models = 未提供任何模型
backends-aliases-label = 别名：

backends-alias-target-title = 别名 → { $target }
backends-alias-disabled-label = { $name }（已禁用）
backends-alias-disabled-title = 裸别名已禁用 — 此后端提供多个模型，请为其指定明确的目标（映射表单）
backends-alias-bare-title = 别名 → 此后端的模型

# Backend CRUD editor (add/edit/delete backends stored in the DB topology).
backends-manage-heading = 管理后端
backends-manage-description = 添加、编辑或删除上游后端。更改会保存到数据库，但只有在您点击“应用更改”后才会生效。
backends-apply-changes = 应用更改
backends-add-heading = 添加后端
backends-field-name = 名称
backends-field-base-url = 基础 URL
backends-field-api-key-env = API 密钥环境变量
backends-field-health-path = 健康检查路径
backends-field-weight = 权重
backends-field-max-inflight = 最大处理中数
backends-field-pool = 池
backends-field-pool-none = （无）
backends-field-pool-hint = 将此后端分配到一个池。位于多个池中的后端会被收敛到此处所选的池。
backends-field-models = 模型（逗号分隔）
backends-field-aliases = 别名（每行 name=target）
backends-field-probe-models = 通过 /models 探测发现模型
backends-field-supports-edit = 支持图像编辑
backends-save-backend = 保存后端
backends-add-backend = 添加后端
backends-delete-backend = 删除
backends-error-name-required = 后端名称为必填项
backends-error-base-url-required = 基础 URL 为必填项
backends-saved = 已保存后端 `{ $name }` — 点击“应用更改”以重新加载
backends-deleted = 已删除后端 `{ $name }` — 点击“应用更改”以重新加载

backends-field-api-key = API 密钥
backends-field-api-key-placeholder = API 密钥（加密存储）
backends-field-api-key-keep = 留空以保留当前密钥

# Duplicate-name guard on the Add-backend form.
backends-error-name-exists = 名为 `{ $name }` 的后端已存在 — 再次点击“添加后端”以覆盖它，或更改名称
backends-overwrite-hint = 该名称已存在。再次保存将覆盖现有后端——其基础 URL、API 密钥、模型、别名和所属池。请更改名称以添加第二个后端。

# An alias that is configured but would not route.
backends-alias-unresolved-title = 已失效：此别名指向 `{ $target }`，而该后端并不提供该模型，因此相关请求会失败。
backends-alias-nothing-title = 已失效：该后端未公布任何模型，裸别名无从绑定。
backends-alias-serves = 实际提供： { $models }
backends-alias-serves-nothing = 当前未提供任何模型。
# Save-time check on the aliases textarea.
backends-alias-target-unknown = 已保存，但该后端不提供这些别名目标：{ $targets } —— 它提供的是 { $models }。在目标完全一致之前，这些别名不会路由。
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
backends-auth-failed = 密钥被拒绝
backends-auth-failed-title = 上游拒绝了健康探测的凭据（401/403），因此模型发现已关闭——不会有新模型通过该后端变为可路由。请检查 API 密钥；若使用环境变量，请确认该变量确实已设置。
backends-no-models-title = 该后端未公布任何模型，因此不会有请求路由到它，裸别名也无从绑定。通常是探测始终未返回数据。
# Maintenance switch.
backends-enabled-label = 接收流量
backends-enabled-hint = 关闭以将该后端排空进行维护。立即生效，无需“应用更改”。其模型仍为已知，因此池中其他后端会接管，客户端只会看到临时不可用，而不是“找不到模型”。
backends-enabled-on = 后端 `{ $name }` 已恢复接收流量
backends-enabled-off = 后端 `{ $name }` 已排空以进行维护 —— 不会再有新请求路由到它
backends-status-drained = 维护中
backends-status-drained-title = 已排空进行维护：该后端可达，但路由器会跳过它。重新打开“接收流量”即可让它回到轮转。
# "Test connection": call the upstream with what is typed in the editor.
backends-test-button = 测试连接
backends-test-hint = 使用上面填写的凭据调用该 URL。不会保存任何内容。
backends-test-insert-hint = 上游公布的模型 id —— 点击即可补全光标所在的别名行：
backends-test-ok = 可达，认证通过（{ $source }），发现 { $count } 个模型。
backends-test-ok-no-models = 可达且认证通过（{ $source }），但响应不是 OpenAI /models 信封，模型发现无法解析。该后端只能提供你在“模型”中列出的 id。
backends-test-auth-failed = 被拒绝，HTTP { $status }：凭据未被接受（{ $source }）。在修好之前模型发现保持关闭，后端不会公布任何模型。
backends-test-http-error = { $url } 返回 HTTP { $status }。
backends-test-unreachable = 无法连接 { $url }：{ $err }
backends-test-timeout = { $url } 在 { $secs } 秒内未响应。
backends-test-key-typed = 使用上面输入的密钥
backends-test-key-stored = 使用已保存的密钥
backends-test-key-env = 来自环境变量 { $var }
backends-test-key-env-unset = 环境变量 { $var } 未设置 —— 请求未携带任何凭据
backends-test-key-none = 未发送凭据
# Where the API key comes from (U5).
backends-key-env-badge = 密钥：环境变量 { $var }
backends-key-env-title = 该后端没有已保存的密钥，而是从这个环境变量读取；该变量当前已设置。
backends-key-env-unset-badge = 环境变量 { $var } 未设置
backends-key-env-unset-title = 该后端没有已保存的密钥，而它指定的环境变量在网关进程中未设置，因此完全不会发送凭据。如果上游需要凭据，每次探测都会得到 401，模型发现保持关闭，后端也不会公布任何模型。请改在“API 密钥”字段中填入密钥，或设置该变量后重启。
# Live name-clash note on the add form (U9).
backends-name-taken = 同名的后端已存在——保存会覆盖它，包括基础 URL、密钥、模型和所属池。请换一个名称以添加第二个后端。

# SPA 后端行：分流开关、每小时负载与删除确认。
backends-drain-button = 停止分流
backends-undrain-button = 恢复分流
backends-requests-per-hour = { $count } 次请求/小时
backends-delete-confirm = 要删除后端 { $name } 吗？之后请应用拓扑。
