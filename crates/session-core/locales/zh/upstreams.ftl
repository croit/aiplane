# STATUS: llm-generated, unreviewed — pending native-speaker QA
# 上游界面：资源池的含义以及看板的列标题。

upstreams-heading = 上游
upstreams-description = 池按类型和选择策略对后端分组。健康状况、负载和提供的模型均实时探测。拓扑更改会保存到数据库，并立即应用于运行中的注册表。
upstreams-add-pool = 池
upstreams-add-backend = 后端
upstreams-unassigned-heading = 未分配
upstreams-unassigned-description = 未分配给任何池的后端。将其加入某个池以向其路由流量。
upstreams-empty = 尚未配置任何池或后端。添加一个池或后端即可开始。
upstreams-cancel = 取消
upstreams-edit-backend = 编辑后端
upstreams-comp-limits = 限额
upstreams-edit-pool = 编辑池

# 站台板：列标题、状态灯词汇、行操作。
board-col-name = 后端
board-col-state = 状态
board-col-load = 负载
board-col-requests = 请求/小时
board-summary-on-time = 准时
board-summary-degraded = 延迟
board-summary-down = 已取消
board-summary-maintenance = 维护中
board-on-time = 准时
board-on-time-detail = 健康，正在处理流量
board-saturated = 已饱和
board-saturated-detail = { $inflight } / { $max } 并发请求占用中
board-down = 已停止
board-down-detail = 健康探测失败
board-maintenance = 维护中
board-maintenance-detail = 已暂停 — 不接受新流量
board-auth = 密钥被拒绝
board-auth-detail = 上游拒绝了 API 密钥
board-no-probe = 无探测
board-no-probe-detail = 尚未探测
board-models-unit = 模型
board-legend-on-time = 准时 — 健康，服务中
board-legend-degraded = 延迟 — 饱和或密钥问题
board-legend-down = 已取消 — 不可达
board-legend-maintenance = 维护 — 已暂停
board-enabled = 服务中
board-drained = 已暂停
board-edit = 编辑

board-detail-endpoint = 端点
board-detail-models = 模型
board-detail-aliases = 别名
board-detail-activity = 流量
board-key-none = 未配置密钥
board-models-none = 未发现
board-models-more = +{ $count } 个
board-pool-account = 账户
board-pool-managed = 此资源池在下方 OpenRouter 账户中管理。

board-filter-placeholder = 筛选后端、模型、别名…
board-filter-clear = 清除
board-filter-empty = 没有后端或资源池匹配此筛选条件。
board-edit-named = 编辑后端 { $name }
upstreams-saved = 已保存 — 立即生效。
board-model-open-catalog = 在目录中打开此模型
