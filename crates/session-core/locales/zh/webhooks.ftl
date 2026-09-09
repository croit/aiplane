# STATUS: llm-generated, unreviewed — pending native-speaker QA

webhooks-heading = Webhook
webhooks-intro = 当外部服务调用某个 URL 时运行提示词。你会获得一个保密的触发 URL；调用方在请求正文中发送的内容会追加到你的提示词后，运行结果会作为一个新的对话打开，你可以在这里阅读。
webhooks-edit-heading = 编辑 Webhook
webhooks-list-empty = 还没有 Webhook。在上方创建一个。

webhooks-name-label = 名称
webhooks-name-placeholder = 例如：部署摘要
webhooks-model-label = 模型
webhooks-model-placeholder = 模型 ID
webhooks-prompt-placeholder = 模型应该如何处理传入的数据？

webhooks-reveal-heading = 你的触发 URL
webhooks-reveal-note = 立即复制——它只显示一次。任何拥有此 URL 的人都可以触发该 Webhook。丢失了？轮换以获取新的 URL。
webhooks-copy = 复制

webhooks-badge-active = 已启用
webhooks-badge-paused = 已暂停
webhooks-mode-sync = 等待响应

webhooks-pause-title = 暂停
webhooks-resume-title = 恢复
webhooks-rotate-title = 轮换密钥
webhooks-edit-title = 编辑
webhooks-delete-title = 删除

# --- 使用不同的提示词重新运行 ---
webhooks-toast-rerun-started = 重新运行完成——正在打开对话……

# --- 运行历史 ---
webhooks-runs-empty = 还没有运行记录。触发该 Webhook 后即可在此查看历史。
webhooks-run-open = 打开对话
webhooks-run-rerun = 重新运行

# SPA-only: the Svelte /webhooks page — inline form, run list, rerun composer.
webhooks-new-heading = 新建 Webhook
webhooks-prompt-untrusted-label = 提示词（负载将作为不可信输入传入）
webhooks-runs-show = 运行记录
webhooks-runs-hide = 隐藏运行记录
webhooks-rerun-prompt-label = 重跑提示词——已保存的负载将通过它重新执行
webhooks-rerun-latest = 重跑最近一次负载
webhooks-rerun-running = 运行中…
webhooks-toast-rerun-failed = 重跑 { $status }
webhooks-rotate-confirm = 生成新的触发密钥？旧的 URL 会立即失效。
webhooks-delete-confirm = 删除此 Webhook？
