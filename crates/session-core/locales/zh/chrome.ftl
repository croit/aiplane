# STATUS: llm-generated, unreviewed — pending native-speaker QA

chrome-theme-toggle-title = 切换主题
chrome-theme-toggle-aria-to-light = 切换到浅色主题
chrome-theme-toggle-aria-to-dark = 切换到深色主题
chrome-lang-switcher-aria = 选择语言

# Web Push turn-complete notifications (server-sent body; `spawn_assistant_worker`).
push-untitled-conversation = 新对话
push-turn-complete-body = 您的回答已就绪。
push-turn-error-body = 该回合以错误结束。

# Web Push：连接器的授权已失效（主动刷新扫描，`tools::mcp::worker`）。
# { $connector } 为连接器显示名称。
push-connector-reconnect-title = 连接需要您重新登录
push-connector-reconnect-body = { $connector } 已断开连接 — 请打开“集成”重新连接。

# SPA-only: the root route, which only routes on to /chat.
chrome-opening-conversations = 正在打开你的对话…

# SPA-only: /login, which exists to bounce straight to the IdP.

searchable-select-search-placeholder = 搜索选项…
searchable-select-search-aria = 搜索{ $field }
searchable-select-clear-search = 清除搜索
searchable-select-no-results = 没有匹配的选项。
searchable-select-model-gdpr = GDPR
searchable-select-model-nda = NDA

# 当某项可选功能在 /admin/settings 中被关闭时，用它代替该页面显示。此时
# 导航项会消失；这是对旧链接或手动输入网址的回应。
feature-disabled-body = 本网关已关闭 { $feature }，因此此页面没有内容可显示。管理员可以在设置中启用它。
feature-disabled-settings-link = 打开设置
