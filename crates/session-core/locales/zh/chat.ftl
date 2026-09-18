# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/mod.rs` — the
# multi-conversation chat page's server-side handlers: page title
# fallback, sidebar/effort/share/pin toasts, and the SSE-toast error
# messages the composer's fetch layer surfaces on failed actions.

chat-default-title = 聊天

chat-error-auth-required = 需要身份验证
chat-error-no-such-turn = 没有此消息
chat-error-db-error = 数据库错误
chat-error-attachments-not-configured = 聊天附件未配置
chat-error-bad-filename = 文件名无效
chat-error-attachment-not-found = 未找到

# SPA-only chat chrome (`web/src/routes/chat/*`): the conversation list,
# the conversation header, the assistant's ask-back card, and the
# turn-status line the server never renders itself.
chat-list-empty = 还没有对话。在上方开始一个吧。
chat-turn-stopped = 已停止
chat-prompt-heading = 助手提问
chat-prompt-placeholder = 输入答案…
chat-prompt-answer = 回答
chat-prompt-skip = 跳过

# 回答生成期间输入框依然可用。
chat-queue-label = { $count ->
   *[other] { $count } 条消息排队中
  }
chat-queue-move-up = 上移
chat-queue-move-down = 下移
chat-queue-edit = 放回输入框
chat-queue-remove = 丢弃
chat-queue-held-title = 附件在页面重新加载时丢失 — 请重新添加附件或丢弃该消息
chat-queue-held-hint = 有一条排队消息在页面重新加载时丢失了附件。在你把它放回输入框并重新添加文件之前，它不会被发送。
chat-queue-was-interjection = 在上一条回答生成时输入，但该回答先结束了
chat-composer-interject = 补充到正在生成的回答
chat-composer-interject-title = 交给正在生成的回答（Ctrl/Cmd+Enter）。会在下一个工具步骤送达；若回答先结束，则作为下一条消息发送。
chat-composer-interrupt = 中断并重新提问
chat-composer-interrupt-title = 停止当前回答，改为发送这条。已经写出的内容仍保留在会话中。
chat-steer-pending = 在本条回答期间补充 — 尚未读取
chat-steer-delivered = 在本条回答期间补充 — 已采纳
chat-steer-resent = 在本条回答期间补充 — 送达太晚，已作为下一条消息发送
chat-steer-discarded = 在本条回答期间补充 — 送达太晚，已丢弃
