# STATUS: llm-generated, unreviewed — pending native-speaker QA

tokens-page-heading = API 令牌
tokens-intro = 用于兼容 OpenAI 的 API 的 Bearer 令牌。明文仅在创建时显示一次——请妥善保存。

tokens-create-heading = 创建令牌
tokens-name-label = 名称
tokens-name-placeholder = 例如 laptop、ci-runner
tokens-ttl-label = 有效期（天）
tokens-create-submit = 创建令牌

tokens-list-heading = 您的令牌
tokens-list-empty = 暂无令牌。请在上方创建一个。

tokens-badge-revoked = 已吊销
tokens-badge-active = 生效中
tokens-remove-button = 移除
tokens-rotate-button = 轮换
tokens-rotate-title = 为此令牌签发新密钥（保留其名称和设置）
tokens-revoke-button = 吊销

tokens-row-meta = 创建于 { $created } · 最近使用 { $last_used } · 过期于 { $expires }
tokens-last-used-never = 从未使用

tokens-tool-use-aria = 工具使用
tokens-tool-use-label = 工具使用

tokens-mcp-allow-description = 需要批准的连接器工具无法通过 API 请求确认；启用后将不经询问直接运行它们。

tokens-minted-heading = 令牌已创建
tokens-minted-copy-warning = 请立即复制该值——之后将无法再次查看。
tokens-copy-aria = 复制令牌
tokens-copy-title = 复制令牌
tokens-minted-name = 名称：{ $name }

tokens-account-user-id-label = 用户 ID

tokens-mcp-ask-enabled-toast = 已为此令牌启用通过 API 使用“ask”模式的 MCP 工具。
tokens-mcp-ask-disabled-toast = 已为此令牌禁用通过 API 使用“ask”模式的 MCP 工具。

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
tokens-models-summary-all = 模型：全部
tokens-models-summary-restricted = 模型：已选 { $count } 个
tokens-models-help = 关闭时，此令牌跟随你自己的访问权限，包括以后新增的模型。开启时，它只能使用你勾选的模型——之后新增的模型在你于此处勾选之前都会被拒绝。
tokens-models-restrict-label = 将此令牌限制为特定模型
tokens-models-save = 保存模型
tokens-models-saved-toast = 令牌已限制为 { $count } 个模型。
tokens-models-cleared-toast = 令牌可使用你的全部模型。
tokens-limits-add = 添加配额
tokens-limits-saved-toast = 令牌配额已保存。

# SPA-only: the Svelte /tokens row panels and their confirm prompts.
tokens-models-heading = 模型白名单
tokens-models-input-placeholder = 模型 id，用逗号分隔
tokens-quota-heading = 配额
tokens-quota-per = 每
tokens-quota-max-placeholder = 上限
tokens-mcp-heading = MCP 连接器
tokens-mcp-allow-button = 允许运行
tokens-mcp-block-button = 阻止运行
tokens-revoke-confirm = 撤销此令牌？正在使用它的客户端会立即失效。
tokens-rotate-confirm = 生成新的密钥？旧密钥会立即失效。
tokens-remove-confirm = 永久删除此令牌记录？
