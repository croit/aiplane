# Strings owned by `gateway/src/rama_server/pages/admin.rs` — `/admin/models` 页面。

admin-heading = 模型
admin-models-routing-heading = 模型与路由
admin-models-routing-intro = 在一个位置配置模型基础设施、模型目录、默认模型和自动路由。
admin-models-tab-upstreams = 上游
admin-models-tab-catalog = 模型目录
admin-models-tab-defaults = 默认模型
admin-models-tab-routing = 自动路由

admin-col-model = 模型

admin-not-configured = 未配置

admin-badge-ctx = 上下文

admin-save-model = 保存模型
admin-edit-model = 编辑
admin-edit-model-page-title = 编辑模型
admin-model-not-found = 找不到该模型，可能已没有后端提供它。
admin-clear-overrides = 清除所有覆盖
admin-cancel = 取消

admin-toml-defaults-label = 采样默认值（TOML）

# 各模型的成本核算价格（每 100 万 token 的价格，输入 / 输出）。
admin-price-in-label = 输入价格
admin-price-out-label = 输出价格
admin-price-in-placeholder = 未定价
admin-price-out-placeholder = 未定价

# 上下文窗口（驱动自动压缩）。
admin-context-window-full-label = 上下文窗口（词元）
admin-context-window-placeholder = 默认

# 各功能的默认模型。
admin-defaults-heading = 默认模型

# 模型能力（三态）+ 回退模型。
admin-cap-tools = 工具

# 网页搜索后端（`search_web` 工具）。
admin-search-heading = 网页搜索
admin-search-provider-label = 提供方
admin-search-provider-searxng = SearXNG（自建）
admin-search-provider-brave = Brave Search API
admin-search-searxng-url-label = SearXNG 基础 URL
admin-search-searxng-url-placeholder = https://searxng.example.com
admin-search-brave-key-label = Brave API 密钥
admin-search-brave-key-placeholder = 留空以保留当前密钥
admin-search-save = 保存网页搜索
admin-search-saved = 网页搜索设置已保存

# ─── SvelteKit 管理端 SPA ─────────────────────────────────────────────────────
# 管理外壳的角色校验、/admin/comfyui 工作流目录，以及旧页面没有的模型编辑器部分。

admin-needs-admin-role = 这些页面需要管理员角色。
admin-overwrite-existing = 覆盖已有条目
admin-comfyui-reload = 重新加载目录
admin-comfyui-reloaded = 目录已重新加载 — 已加载 { $count } 个工作流。
admin-comfyui-empty = 未加载任何工作流——请检查内容目录。
admin-comfyui-heading = ComfyUI 工作流目录
admin-comfyui-page-title = ComfyUI — 工作流目录
admin-comfyui-intro = 无界面的 ComfyUI 工作节点。网关将每个已加载工作流公开为模型可调用的 comfyui_<id> 工具，用户不会直接看到 ComfyUI。
admin-comfyui-not-configured = 未配置
admin-comfyui-not-configured-help = 在设置中启用 ComfyUI，填写基础 URL 和工作流目录，然后重启网关以加载目录。
admin-comfyui-operator-config = 运维配置
admin-comfyui-worker-url = Worker 基础 URL
admin-comfyui-content-directory = 内容目录
admin-comfyui-timeout = 工作流超时
admin-comfyui-poll-interval = 队列轮询间隔
admin-comfyui-config-help = 内容目录由运维人员管理，不属于公开仓库。在其中编辑清单后点击上方的重新加载，无需重启。
admin-comfyui-loaded-workflows = 已加载工作流
admin-comfyui-node = 节点 { $id }
admin-comfyui-parameters = 参数
admin-comfyui-required = 必填
admin-comfyui-recent-jobs = 最近任务
admin-comfyui-reloaded-skipped = 目录已重新加载 — 已加载 { $count } 个工作流，跳过 { $skipped } 个。
admin-comfyui-max-concurrent = 并发任务数
admin-comfyui-tab-workflows = 工作流
admin-comfyui-tab-jobs = 运行记录
admin-comfyui-jobs-page-title = ComfyUI — 运行记录
admin-comfyui-jobs-intro = 模型最近运行的全部工作流，最新在前：产出了什么、耗时多久，以及发起请求的对话。
admin-comfyui-jobs-window = 网关记录的最近 { $count } 次运行。
admin-comfyui-jobs-empty = 暂无运行记录。
admin-comfyui-worker-status = 工作节点
admin-comfyui-worker-reachable = 可访问
admin-comfyui-worker-unreachable = 无法访问
admin-comfyui-worker-checking = 检查中…
admin-comfyui-worker-queue = { $running } 运行中 · { $pending } 排队中
admin-comfyui-worker-software = ComfyUI { $version } · Python { $python } · PyTorch { $torch }
admin-comfyui-worker-vram = { $total } 中剩余 { $free }
admin-comfyui-search-placeholder = 搜索工作流与参数
admin-comfyui-search-empty = 没有匹配的工作流。
admin-comfyui-filename-prefix = 输出前缀
admin-comfyui-required-count = { $total } 项中 { $required } 项必填
admin-comfyui-no-params = 此工作流没有参数。
admin-comfyui-detail-empty = 选择一个工作流，查看模型看到的调用约定。
admin-comfyui-param-column = 参数
admin-comfyui-param-description-column = 说明
admin-comfyui-filter-all = 全部
admin-comfyui-filter-completed = 已完成
admin-comfyui-filter-pending = 进行中
admin-comfyui-filter-failed = 失败
admin-comfyui-stats-heading = 各工作流可靠性
admin-comfyui-col-workflow = 工作流
admin-comfyui-col-runs = 运行次数
admin-comfyui-col-failed = 失败
admin-comfyui-col-median = 中位数
admin-comfyui-col-status = 状态
admin-comfyui-col-duration = 耗时
admin-comfyui-col-when = 开始时间
admin-comfyui-col-result = 结果
admin-comfyui-job-open = 打开对话
admin-comfyui-job-still-running = 仍在运行
admin-comfyui-refresh = 刷新
admin-clear-overrides-confirm = 要删除 { $model } 的全部已保存覆盖设置吗？
admin-cap-no-fallback = （无）

admin-page-title = 模型 — AIplane

admin-no-models = 尚未发现任何模型。一旦有可用的上游后端，它就会显示在这里。

admin-filter-placeholder = 筛选模型…

admin-filter-all = 全部

admin-filter-chat = 聊天

admin-filter-other = 其他类型

admin-filter-aliases = 别名

admin-filter-configured = 仅已配置

admin-col-kind = 类型

admin-col-price = 价格 入/出

admin-col-context = 上下文

admin-col-reasoning = 推理

admin-col-configured = 已配置

admin-value-default = 默认

admin-value-na = 不适用

admin-alias-inherits = 继承目标的设置

admin-reasoning-auto-resolved = 自动 → { $style }

admin-badge-price = 价格

admin-badge-budget = 预算

admin-badge-caps = 能力

admin-badge-toml = TOML

admin-other-price-note = 采样、推理和上下文不适用于此类型 —— 仅价格用于成本核算。

admin-toml-placeholder-header = # 常用键（vLLM/OpenAI）：

admin-reasoning-style-label = 推理风格

admin-reasoning-style-aria = 推理风格

admin-reasoning-auto = 自动

admin-reasoning-none = 无

admin-reasoning-qwen = Qwen（vLLM）

admin-reasoning-openai = OpenAI

admin-reasoning-glm = GLM / z.AI

admin-reasoning-anthropic = Anthropic
admin-reasoning-ollama = Ollama

admin-effort-standard = 标准

admin-effort-deep = 深度

admin-effort-max = 最大

admin-budget-placeholder = 默认

admin-budget-hint = 每个强度级别的最大思考 token 数。留空 = 后端默认值（不设上限）。“Fast” 会禁用推理。

admin-effort-default-option = （默认）

admin-effort-hint = 每个强度级别的推理强度。留空 = 内置默认值。“Fast” 会禁用推理。

admin-saved-model = 已保存 `{ $model }` —— 立即生效

admin-cleared-defaults = 已清除 `{ $model }` 的覆盖

admin-price-label = { $cur }/{ $unit }

admin-price-unit-tokens = 100 万令牌

admin-price-unit-images = 图像

admin-price-unit-characters = 字符

admin-price-unit-seconds = 秒

admin-alias-chip = 别名

admin-defaults-intro = 选择每项功能预先选中的模型。留空 = 第一个可用模型（旧行为）。

admin-defaults-chat-label = 聊天

admin-defaults-voice-label = 语音（转录）

admin-defaults-image-label = 图像生成

admin-defaults-embedding-label = 嵌入（RAG）

admin-defaults-first-option = 第一个可用

admin-defaults-saved = 默认模型已设置为 `{ $model }`

admin-defaults-cleared = 默认模型已清除

admin-capabilities-heading = 能力

admin-cap-vision = 视觉

admin-cap-structured-output = 结构化输出

admin-cap-audio-input = 音频输入

admin-cap-pdf-input = PDF 输入

admin-cap-parallel-tools = 并行工具

admin-cap-unknown = 未知

admin-cap-enabled = 启用

admin-cap-disabled = 禁用

admin-cap-fallback-vision = 视觉回退

admin-cap-fallback-tools = 工具回退

admin-search-intro = 由哪个后端响应助手的 `search_web` 工具。SearXNG 只需一个基础 URL，如果你自建实例则每次查询不产生费用；Brave 需要 API 密钥。密钥会加密存储。

admin-search-brave-key-set = 已存储密钥（加密）。

admin-search-brave-key-unset = 未存储密钥。

admin-search-brave-key-clear = 删除已存储的密钥

# 上下文窗口的来源。
admin-context-detected = 已检测：{ $window } 词元
admin-context-unreported = 此后端未报告该模型的上下文窗口。请在此填写，或在服务器上确认。
admin-context-exceeds-detected = 此后端报告为 { $window } 词元。更大的值不会被压缩，服务器会静默截断。
auto-route-heading = 自动模型路由
auto-route-description = 提供标准模型别名，为每个请求选择最佳的可用模型。
auto-route-add = 添加自动路由
auto-route-privacy = 选择器会接收分类所需的请求内容，即使最终选中的生成模型在本地运行。
auto-route-empty = 尚未配置自动路由。
auto-route-needs-selector = 创建自动路由前，请先添加 System One 上游。
auto-route-needs-candidates = 创建自动路由前，请先添加至少两个不同的聊天模型。
auto-route-open-upstreams = 打开上游配置
auto-route-candidates = 候选模型
auto-route-edit = 编辑
auto-route-delete = 删除
auto-route-delete-confirm = 删除自动路由“{ $alias }”吗？
auto-route-deleted = 已删除自动路由“{ $alias }”。
auto-route-saved = 已保存自动路由“{ $alias }”。
auto-route-alias = 模型别名
auto-route-alias-help = 客户端在标准 model 字段中使用此值。
auto-route-selector = 选择器模型
auto-route-selector-help = 来自 System One 上游、用于选择候选键的模型。
auto-route-editor-help = 请先使用影子模式并检查路由决策，确认策略就绪后再启用路由。
auto-route-objective = 优化目标
auto-route-objective-quality = 质量
auto-route-objective-balanced = 均衡
auto-route-objective-cost = 成本
auto-route-rollout = 发布模式
auto-route-rollout-shadow = 影子模式（仅测量）
auto-route-rollout-active = 启用
auto-route-rollout-help = 影子模式记录决策，但流量仍发送到回退目标；启用后将应用选择器的决策。
auto-route-confidence = 最低置信度
auto-route-confidence-help = 置信度低于此值时使用回退目标。
auto-route-timeout = 选择器超时（毫秒）
auto-route-instructions = 路由说明
auto-route-instructions-help = 用自然语言为选择器提供可选规则，不需要特殊语法。
auto-route-instructions-placeholder = 示例：复杂分析和代码优先选择 expert；简短的常规请求使用 fast。
auto-route-candidate-add = 添加候选项
auto-route-candidates-help = 请按适合处理的请求描述每个模型。选择器只会看到候选键和描述，不会看到目标模型名称。
auto-route-candidate-key = 不透明键
auto-route-candidate-target = 目标模型或静态别名
auto-route-candidate-description = 何时使用此候选项
auto-route-candidate-description-help = 必填自由文本。说明此候选项最适合处理哪些请求；选择器会看到这段文字，但看不到目标模型名称。
auto-route-candidate-description-placeholder = 示例：重视低延迟和成本的简短常规请求。
auto-route-fallback = 回退目标
auto-route-session-affinity = 会话保持使用首次选中的模型
auto-route-session-affinity-help = 在 TTL 到期前，为同一已认证客户端和会话重复使用实际目标。
auto-route-session-ttl = 会话亲和 TTL（秒）
auto-route-cancel = 取消
auto-route-save = 保存路由
auto-route-saving = 正在保存…
auto-route-decisions = 最近的路由决策
auto-route-result = 实际目标
auto-route-latency = 选择器延迟
auto-route-reason-selected = 已选择
auto-route-reason-shadow = 影子建议
auto-route-reason-low-confidence = 低置信度
auto-route-reason-selector-error = 选择器错误
auto-route-reason-session-affinity = 会话亲和性
auto-route-error-unavailable = 当前没有已配置的目标能够处理此请求。请检查候选模型的可用性和能力，然后重试。
auto-route-error-internal = 无法加载自动路由。请检查网关日志后重试。
auto-route-error-invalid-body = 自动路由请求格式错误。请检查提交的字段后重试。
auto-route-error-invalid-config = 自动路由配置无效。请检查候选模型、回退模型、置信度和超时设置。
auto-route-error-nested = 自动路由不能指向其他自动路由。请改用静态模型别名或模型 ID。
auto-route-error-missing-alias = 请求 URL 中缺少自动路由别名。
auto-route-error-not-found = 自动路由不存在。请刷新路由列表后重试。
