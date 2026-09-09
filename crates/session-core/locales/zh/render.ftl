# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `session-core/src/render.rs` — the HTML renderers for
# the chat-style session UI (conversation bubbles, tool-call rows, the
# document canvas, and the composer). Driver-agnostic: both the gateway
# and any future consumer of this crate render through these functions.

render-edit-button = ✎ 编辑

render-retry-button = ↻ 重试

render-attachment-remove-aria = 删除附件

# 回复中代码块上的复制按钮（仅图标，因此这是其提示文本 / 无障碍名称）。

render-thinking-spinner = 思考中…
render-thinking-finalized = 思考了 { $secs } 秒

render-tool-status-used = 已使用

render-canvas-edit-button = ✎ 编辑
render-canvas-save = 另存为新版本
render-canvas-cancel = 取消

render-composer-attach-aria = 添加附件
render-composer-attach-title = 添加附件（也可拖放/粘贴）
render-composer-send = 发送
render-composer-stop = 停止

# The browser prompt behind the ✎ Edit button, and the per-file title on
# an attachment's remove button.
render-edit-prompt = 编辑你的消息：
render-attachment-remove-title = 移除 { $filename }
