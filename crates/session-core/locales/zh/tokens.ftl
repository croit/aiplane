# STATUS: llm-generated, unreviewed — pending native-speaker QA

tokens-page-heading = API 令牌
tokens-intro = 用于连接 AIplane 的应用和代码的令牌。完整令牌仅在创建或轮换时显示，请复制并妥善保管。

tokens-create-heading = 创建令牌
tokens-name-label = 名称
tokens-name-placeholder = 例如 laptop、ci-runner
tokens-ttl-label = 有效期（天）
tokens-create-submit = 创建令牌

tokens-list-heading = 您的令牌
tokens-list-empty = 暂无令牌。请用上方的按钮创建一个。

tokens-badge-revoked = 已吊销
tokens-badge-active = 生效中
tokens-remove-button = 移除
tokens-rotate-button = 轮换
tokens-rotate-title = 为此令牌签发新密钥（保留其名称和设置）
tokens-revoke-button = 吊销

tokens-row-meta = 创建于 { $created } · 最近使用 { $last_used } · 过期于 { $expires }
tokens-last-used-never = 从未使用

tokens-tool-use-label = 工具使用

tokens-mcp-allow-description = 需要批准的连接器工具无法通过 API 请求确认；启用后将不经询问直接运行它们。

tokens-minted-heading = 令牌已创建
tokens-minted-copy-warning = 请立即复制该值——之后将无法再次查看。
tokens-copy-aria = 复制令牌
tokens-minted-name = 名称：{ $name }

tokens-account-user-id-label = 用户 ID

# Web Push "turn complete" opt-in card (rendered by `render_push_card`; wired
# client-side by `ui/ts/push.ts`). Device-local notification settings.
tokens-push-enable = 在此设备上启用
tokens-push-disable = 在此设备上停用
tokens-push-on = 此设备已开启通知。
tokens-push-enabled = 已在此设备上启用通知。
tokens-push-disabled = 已在此设备上停用通知。
tokens-push-error = 无法更改通知设置。

# 每个令牌的用量、模型白名单与配额（/tokens）。
tokens-usage-line = 本月：{ $requests } 次请求 · { $tokens } tokens · { $cost }
tokens-models-summary-restricted = 模型：已选 { $count } 个
tokens-models-help = 关闭时，此令牌跟随你自己的访问权限，包括以后新增的模型。开启时，它只能使用你勾选的模型——之后新增的模型在你于此处勾选之前都会被拒绝。
tokens-models-restrict-label = 将此令牌限制为特定模型
tokens-models-saved-toast = 令牌已限制为 { $count } 个模型。
tokens-models-cleared-toast = 令牌可使用你的全部模型。
tokens-limits-add = 添加配额
tokens-limits-saved-toast = 令牌配额已保存。

# SPA-only: the Svelte /tokens row panels and their confirm prompts.
tokens-quota-max-placeholder = 上限
tokens-revoke-confirm = 撤销此令牌？正在使用它的客户端会立即失效。
tokens-rotate-confirm = 生成新的密钥？旧密钥会立即失效。
tokens-remove-confirm = 永久删除此令牌记录？

tokens-create-description = 为兼容 OpenAI 的 API 生成一个新的 Bearer 令牌。
tokens-tool-use-description = 允许此令牌调用网关工具（网络搜索、RAG 等）。
tokens-capabilities-summary = 能力
tokens-panel-close = 关闭
tokens-edit-button = 编辑
tokens-mcp-allow-label = 允许通过 API 使用“ask”模式的 MCP 工具
tokens-account-heading = 账户
tokens-signed-in-as = 已登录为 { $email }
tokens-account-oidc-label = OIDC 角色
tokens-account-rbac-label = RBAC 角色 ID
tokens-roles-none = 无
tokens-roles-none-granted = 未授予任何角色
tokens-push-heading = 通知
tokens-push-description = 当您发起的回答在您离开应用时完成，在此设备上收到通知。
tokens-push-off = 此设备已关闭通知。
tokens-push-denied = 此浏览器已阻止通知。请在浏览器设置中允许以启用。
tokens-push-unsupported = 此浏览器不支持通知。
tokens-models-none-picked = 请至少勾选一个模型，或关闭该限制。
tokens-limits-help = 仅针对此令牌的上限。你自己的预算仍然有效，因此这只会收紧该令牌的用量，绝不会放宽。
tokens-limits-remove = 移除
tokens-limits-admin-badge = 由管理员设置
tokens-models-admin-set = 运营方还将此令牌限制为：{ $models }。你的选择只能在此基础上进一步收紧，无法放宽。

# Client setup guides and token-page tabs.
tokens-tab-tokens = 令牌
tokens-tab-guides = 设置指南
tokens-tab-account = 账户
tokens-guides-heading = 设置客户端
tokens-guides-intro = 选择下面的应用并按步骤操作。一个令牌可以用于多个应用，但每个应用使用单独的令牌更方便管理。
tokens-guides-before = 先在“令牌”标签页创建并复制令牌。完整令牌只显示一次，请妥善保管。
tokens-guide-opencode = OpenCode
tokens-guide-claude = Claude Code
tokens-guide-python = Python (OpenAI)
tokens-opencode-step-1 = 在“令牌”标签页创建并复制令牌。
tokens-opencode-step-2 = 在 OpenCode 中运行 /connect，选择 Other，输入 aiplane 作为提供商 ID，然后粘贴令牌。
tokens-opencode-step-3 = 将此配置保存到 ~/.config/opencode/opencode.jsonc。把 YOUR_MODEL_ID 替换为可用的聊天模型 ID。
tokens-opencode-finish = 运行 opencode，然后用 /models 选择 aiplane/YOUR_MODEL_ID。
tokens-claude-step-1 = 在“令牌”标签页创建并复制令牌。
tokens-claude-step-2 = 选择可用的聊天模型 ID。管理员可能已为 Claude Code 设置别名。
tokens-claude-step-3 = 将这些命令粘贴到终端。先替换令牌和模型 ID。
tokens-claude-finish = Claude Code 使用不带 /v1 的基础 URL。如果提示找不到模型，请向管理员询问应使用哪个别名。
tokens-python-step-1 = 在“令牌”标签页创建并复制令牌。
tokens-python-step-2 = 运行 python -m pip install openai 安装 OpenAI Python 包，然后在终端将 OPENAI_API_KEY 设为你的令牌。
tokens-python-step-3 = 将示例保存为 chat.py。把 YOUR_MODEL_ID 替换为可用的聊天模型 ID。
tokens-python-finish = 运行 python chat.py。请将令牌保存在环境变量中，不要写入脚本。
tokens-guide-omp = Oh My Pi
tokens-guide-pi = Pi
tokens-omp-finish = 运行 omp，然后使用 /model 选择 aiplane/YOUR_MODEL_ID。
tokens-omp-step-1 = 在“令牌”标签页创建并复制令牌。
tokens-omp-step-2 = 将此配置保存到 ~/.omp/agent/models.yml。请先替换令牌和 YOUR_MODEL_ID，并将 contextWindow 设为模型的上下文大小。
tokens-pi-finish = 运行 pi，然后使用 /model 选择 aiplane/YOUR_MODEL_ID。
tokens-pi-step-1 = 在“令牌”标签页创建并复制令牌。
tokens-pi-step-2 = 将此配置保存到 ~/.pi/agent/models.json。请先替换令牌和 YOUR_MODEL_ID。
tokens-guides-model-note = 需要模型 ID？可在聊天模型选择器中查看可用模型，也可以带上令牌请求 GET /v1/models。

notifications-loading = 正在加载通知设置…
notifications-unavailable = 此网关目前无法使用通知。请让管理员检查推送设置。
notifications-admin-settings-link = 打开推送设置
tokens-tile-models = 模型
tokens-tile-tools = 工具
tokens-tile-budget = 预算
tokens-save = 保存
tokens-menu-aria = 令牌操作
tokens-expires-today = 今天到期
tokens-expires-soon = { $days } 天后到期
tokens-models-tile-all = 全部 { $count } 个模型 — 跟随你的访问权限
tokens-models-tile-some = { $total } 个模型中的 { $count } 个
tokens-models-tile-compliant = 全部符合 GDPR 且受 NDA 保护
tokens-models-tile-noncompliant = { $count } 个模型不受 GDPR 或 NDA 保护
tokens-models-tile-admin = 运维人员允许其中 { $count } 个
tokens-models-max-price = 最高每百万输出 token { $price }
tokens-models-noncompliant-warning = 这里有 { $count } 个模型不符合 GDPR 或不受 NDA 保护。不要通过此令牌发送个人数据或机密材料。
tokens-models-search = 搜索模型
tokens-models-filter-gdpr = 符合 GDPR
tokens-models-filter-nda = 受 NDA 保护
tokens-models-filter-free = 免费
tokens-models-select-compliant = 选择所有 GDPR + NDA 模型
tokens-models-select-none = 清除选择
tokens-models-empty = 没有匹配的模型。
tokens-models-alias = { $target } 的别名
tokens-models-gdpr-ok = 符合 GDPR：个人数据受保护。
tokens-models-nda-ok = 受保密协议保护。
tokens-models-price-free = 免费
tokens-models-price-tokens = 每百万 输入 { $input } / 输出 { $output }
tokens-models-price-per-images = 每张图片 { $price }
tokens-models-price-per-characters = 每字符 { $price }
tokens-models-price-per-seconds = 每秒 { $price }
tokens-models-kind-chat = 对话
tokens-models-kind-transcription = 语音转文字
tokens-models-kind-speech = 文字转语音
tokens-models-kind-embedding = 嵌入
tokens-models-kind-rerank = 重排序
tokens-models-kind-image = 图像
tokens-models-kind-system_one = System One
tokens-tools-tile-on = 已允许
tokens-tools-tile-off = 关闭
tokens-tools-tile-pinned = { $count } 个始终开启
tokens-tools-tile-mcp-allowed = “Ask” MCP 工具无需确认即运行
tokens-tools-tile-mcp-blocked = “Ask” MCP 工具已阻止
tokens-tools-capabilities-help = 此令牌可使用的工具和技能。
tokens-tools-saved-toast = 令牌工具已保存。
tokens-budget-tile-none = 没有单独限额
tokens-budget-tile-more = 另有 { $count } 个
tokens-budget-owner-applies = 你自己的预算同样适用
tokens-budget-token-heading = 此令牌
tokens-budget-owner-heading = 同样适用：你自己的预算
tokens-budget-owner-none = 你没有预算限额。
tokens-budget-dimension = 限制内容
tokens-budget-window = 周期
