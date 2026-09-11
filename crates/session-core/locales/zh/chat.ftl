# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/mod.rs` — the
# multi-conversation chat page's server-side handlers: page title
# fallback, sidebar/effort/share/pin toasts, and the SSE-toast error
# messages the composer's fetch layer surfaces on failed actions.

chat-default-title = 聊天

chat-error-still-streaming = 该用户仍有响应正在生成 — 请等待或点击停止。

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
