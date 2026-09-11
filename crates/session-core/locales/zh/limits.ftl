# 管理端速率限制 / 配额编辑器 (/admin/limits)。
limits-heading = 速率限制与配额
limits-add-heading = 添加或更新限制
limits-field-subject = 适用于
limits-field-model = 模型
limits-field-dimension = 限制
limits-field-window = 每
limits-field-value = 数值
limits-add-submit = 保存限制
limits-subject-global = 所有人（默认）
limits-subject-role = 角色
limits-subject-user = 用户
limits-dim-requests = 请求数
limits-dim-tokens = 令牌数
limits-dim-cost = 成本 ({ $cur })
limits-dim-cost-short = 成本
limits-win-hour = 小时
limits-win-day = 天
limits-win-week = 周
limits-win-month = 月
limits-col-subject = 适用于
limits-col-scope = 模型
limits-col-limit = 限制
limits-col-window = 时间窗口
limits-none = 未配置任何限制 — 所有人都不受限制。
limits-all-models = 所有模型
limits-delete = 删除
limits-saved = 已保存 { $subject } 的限制
limits-subject-token = API 令牌

# SPA 规则表：标题、“来源”列与删除确认。
limits-delete-confirm = 要删除此规则吗？
limits-intro = 限制调用方在滑动时间窗口内可使用的请求数、令牌数或花费金额。规则按最具体优先解析：用户自身的规则优先，否则取其角色中最宽松的，否则使用全局默认值。若无任何规则，则所有人都不受限制。针对 API 令牌的规则是一道额外上限，与其所有者的额度一并检查，因此只会收紧该令牌的用量。仅计入计费池（自托管且 enforce_limits = false 的池不计入），且用户的整个额度在其 API 令牌、聊天和计划任务之间共享。
limits-field-subject-id = 角色 / 用户 / 令牌
limits-field-subject-id-ph = 角色 id、用户邮箱或令牌 id
limits-col-value = 数值
limits-col-actions = 操作
limits-deleted = 已移除限制
