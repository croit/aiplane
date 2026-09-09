# STATUS: llm-generated, unreviewed — pending native-speaker QA

backends-heading = 上游后端

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
backends-add-backend = 添加后端
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
backends-drain-button = 停止分流
backends-undrain-button = 恢复分流
backends-requests-per-hour = { $count } 次请求/小时
backends-delete-confirm = 要删除后端 { $name } 吗？之后请应用拓扑。
