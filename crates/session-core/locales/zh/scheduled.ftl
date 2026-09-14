# STATUS: llm-generated, unreviewed — pending native-speaker QA


scheduled-heading = 定时操作
scheduled-intro = 按计划自动运行提示词。每次运行都会打开一个新对话，您可以在此查看——选择模型、编写提示词，并设置运行时间。
scheduled-create-submit = 创建定时操作
scheduled-list-heading = 您的定时操作
scheduled-list-empty = 还没有定时操作。请用上方的按钮创建一个。

scheduled-back = 返回
scheduled-edit-heading = 编辑定时操作
scheduled-save-submit = 保存更改

scheduled-name-label = 名称
scheduled-name-placeholder = 例如：每日新闻摘要
scheduled-model-label = 模型
scheduled-model-placeholder = 模型 ID（例如 gpt-4o-mini）
scheduled-gdpr-warning = 该模型不符合 GDPR 合规要求。定时运行会自动向其发送您的提示词——请避免包含个人数据。
scheduled-nda-warning = 该模型未受保密协议保护。请勿向该模型发送受 NDA 保护或专有的内容。
scheduled-prompt-label = 提示词
scheduled-prompt-placeholder = 每次运行时模型应该做什么？
scheduled-tools-toggle-label = 允许使用工具（网页搜索、RAG、附件）——与聊天中相同
scheduled-reuse-toggle-label = 复用上次运行的对话——每次运行都会延续同一对话
scheduled-reuse-rounds-prefix = 发送最近
scheduled-reuse-rounds-aria = 要重放的历史轮数
scheduled-reuse-rounds-suffix = 轮

scheduled-builder-heading = 计划
scheduled-mode-hourly = 每小时
scheduled-mode-daily = 每天
scheduled-mode-weekly = 每周
scheduled-mode-monthly = 每月
scheduled-mode-advanced = 高级
scheduled-weekday-mon = 周一
scheduled-weekday-tue = 周二
scheduled-weekday-wed = 周三
scheduled-weekday-thu = 周四
scheduled-weekday-fri = 周五
scheduled-weekday-sat = 周六
scheduled-weekday-sun = 周日
scheduled-on-day-label = 在第几天
scheduled-of-every-month = 每月
scheduled-at-label = 在
scheduled-hour-aria = 小时
scheduled-minute-aria = 分钟
scheduled-of-every-hour = 每小时
scheduled-timezone-label = 时区
scheduled-cron-label = Cron 表达式
scheduled-cron-help = 五个字段：分钟 小时 日 月 星期。

scheduled-no-upcoming-runs = 没有即将运行的任务。
scheduled-next-runs-prefix = 接下来的运行：{ " " }

scheduled-err-pick-weekday = 请至少选择一个星期。
scheduled-err-enter-cron = 请输入 cron 表达式。


scheduled-toast-not-found = 没有此定时操作。

scheduled-badge-active = 已启用
scheduled-badge-paused = 已暂停
scheduled-status-paused = 已暂停
scheduled-next-run = 下次运行：{ $when }
scheduled-no-upcoming-run = 没有即将运行的任务
scheduled-last-success = 上次：✓ { $when }
scheduled-last-failure = 上次：✗ { $when }
scheduled-pause-title = 暂停
scheduled-resume-title = 恢复
scheduled-edit-title = 编辑
scheduled-delete-title = 删除
scheduled-delete-confirm = 删除此计划操作？
scheduled-preview-summary = { $summary }（{ $timezone }）
scheduled-preview-next-runs = 下次运行：{ $runs }
scheduled-last-error = 上次错误：{ $error }

# 列表页指向计划任务所产出内容的链接，以及其背后的运行历史
# （`/scheduled/{id}/runs`）。
scheduled-create-heading = 新建计划任务
scheduled-new-page-title = 新建计划任务
scheduled-edit-named-heading = 编辑 { $name }
scheduled-toast-created = 已创建计划任务。
scheduled-toast-saved = 已保存计划任务。
scheduled-badge-reuses-chat = 同一个对话
scheduled-open-chat = 打开对话
scheduled-open-chats = { $count } 个对话
scheduled-open-runs = { $count } 次运行
scheduled-never-run = 尚未运行
scheduled-runs-page-title = 运行记录
scheduled-runs-heading = 运行记录 · { $name }
scheduled-runs-intro = 全部已记录的触发，最新在前，并附上它打开的对话。
scheduled-runs-empty = 还没有运行记录。该任务自创建以来尚未触发。
scheduled-run-open = 打开对话
scheduled-run-status-ok = 成功
scheduled-run-status-error = 错误
scheduled-run-status-pending = 运行中
