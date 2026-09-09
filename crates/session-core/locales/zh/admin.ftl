# Strings owned by `gateway/src/rama_server/pages/admin.rs` — `/admin/models` 页面。

admin-heading = 模型

admin-col-model = 模型

admin-not-configured = 未配置

admin-badge-ctx = 上下文

admin-save-model = 保存模型
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
admin-comfyui-reloaded = 已重新加载 { $count } 个工作流。
admin-comfyui-empty = 未加载任何工作流——请检查内容目录。
admin-defaults-model-aria = { $feature } 的默认模型
admin-defaults-set = 设置
admin-search-provider-none = 无
admin-add-overrides-heading = 添加模型覆盖设置
admin-edit-model-heading = 编辑 { $model }
admin-add-model = 添加…
admin-pricing-unit-label = 计价单位
admin-pricing-unit-mtok = 每 Mtok
admin-pricing-unit-ktok = 每 Ktok
admin-pricing-unit-kimgs = 每 1000 张图片
admin-clear-overrides-confirm = 要删除 { $model } 的全部已保存覆盖设置吗？
admin-users-col-email = 电子邮件
