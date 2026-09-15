# STATUS: llm-generated, unreviewed — pending native-speaker QA


backends-status-down = 离线
backends-status-up = 正常

backends-inflight-label = 处理中 { $load }

# Backend CRUD editor (add/edit/delete backends stored in the DB topology).
backends-apply-changes = 应用更改
backends-field-name = 名称
backends-field-base-url = 基础 URL
backends-field-pool = 池
backends-field-pool-none = （无）
backends-save-backend = 保存后端
backends-delete-backend = 删除

backends-field-api-key = API 密钥
backends-field-api-key-keep = 留空以保留当前密钥

# An alias that is configured but would not route.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.
backends-status-drained = 维护中

# SPA 后端行：分流开关、每小时负载与删除确认。
backends-add-heading = 添加后端
backends-name-taken = 同名的后端已存在——保存会覆盖它，包括基础 URL、密钥、模型和所属池。请换一个名称以添加第二个后端。
backends-field-api-key-placeholder = API 密钥（加密存储）
backends-field-api-key-env = API 密钥环境变量
backends-field-health-path = 健康检查路径
backends-field-pool-hint = 将此后端分配到一个池。位于多个池中的后端会被收敛到此处所选的池。
backends-field-weight = 权重
backends-field-max-inflight = 最大处理中数
backends-field-models = 模型（逗号分隔）
backends-field-aliases = 别名（每行 name=target）
backends-field-probe-models = 通过 /models 探测发现模型
backends-field-supports-edit = 支持图像编辑
backends-status-saturated = 已饱和
backends-auth-failed-title = 上游拒绝了健康探测的凭据（401/403），因此模型发现已关闭——不会有新模型通过该后端变为可路由。请检查 API 密钥；若使用环境变量，请确认该变量确实已设置。
backends-auth-failed = 密钥被拒绝
backends-no-models-title = 该后端未公布任何模型，因此不会有请求路由到它，裸别名也无从绑定。通常是探测始终未返回数据。
backends-no-models = 未提供任何模型
backends-key-env-badge = 密钥：环境变量 { $var }
backends-key-env-unset-badge = 环境变量 { $var } 未设置
backends-enabled-hint = 关闭以将该后端排空进行维护。立即生效，无需“应用更改”。其模型仍为已知，因此池中其他后端会接管，客户端只会看到临时不可用，而不是“找不到模型”。
backends-enabled-label = 接收流量
backends-activity-summary = 15分钟 { $m15 } · 30分钟 { $m30 } · 60分钟 { $m60 }
backends-aliases-label = 别名：
backends-alias-target-title = 别名 → { $target }
backends-alias-disabled-title = 裸别名已禁用 — 此后端提供多个模型，请为其指定明确的目标（映射表单）
backends-alias-disabled-label = { $name }（已禁用）
backends-fallback-offline-title = fallback_offline：当此池中某已知模型的所有后端都离线时使用
backends-fallback-offline-badge = 离线 ↩ { $model }
backends-pool-empty = 此池中没有后端。

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
backends-error-base-url-required = 基础 URL 为必填项

backends-parallel-mismatch = 服务器同时处理 { $parallel } 个
backends-detect-profile = 已识别：{ $profile }
