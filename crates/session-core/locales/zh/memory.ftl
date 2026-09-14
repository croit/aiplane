# STATUS: llm-generated, unreviewed — pending native-speaker QA

memory-heading = 记忆
memory-description = 助手记住的关于你的内容，按类型分组。在这里添加、编辑或删除条目——这是你账户的记忆，完全由你掌控。是否启用该功能可在“工具”页面切换。

memory-add-heading = 添加一条记忆
memory-kind-aria = 记忆类型
memory-content-placeholder = 例如：偏好使用公制单位回答

memory-empty = 这里还没有内容。
memory-save-button = 保存
memory-delete-title = 删除记忆

# SPA-only: the Svelte /memory page's inline add/edit form.
memory-kind-preference = 偏好
memory-kind-project = 项目背景
memory-kind-fact = 事实
memory-content-label = 内容
memory-add-button = 记住
memory-delete-confirm = 删除这条记忆？

# The per-category Add button in each card header; it opens the add dialog
# with that card's kind already selected.
memory-add-short = 添加

# One line under each card heading saying how that kind reaches the
# assistant: preferences ride in the system context of every conversation,
# while project notes and facts wait to be looked up with `recall`. The
# difference changes which bucket a user files something in, and nothing
# else on the page reveals it.
memory-kind-preference-hint = 始终在上下文中——从每次对话的第一条消息起就一并发送，助手无需提醒即会遵循。
memory-kind-project-hint = 按需读取——当对话涉及你的工作时，助手才会查阅这些条目。
memory-kind-fact-hint = 按需读取——当这些条目变得相关时，助手才会查阅。
