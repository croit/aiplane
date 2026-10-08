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
chat-error-attachment-storage-failed = 附件存储未正确响应，因此无法获取该文件。请重试；如果仍然失败，请管理员在 /admin/settings 中检查附件存储。
chat-error-turn-interrupted = 内部错误中断了此次回复。请重试。

# SPA-only chat chrome (`web/src/routes/chat/*`): the conversation list,
# the conversation header, the assistant's ask-back card, and the
# turn-status line the server never renders itself.
chat-list-empty = 还没有对话。在上方开始一个吧。
chat-turn-stopped = 已停止
chat-waiting-approval = 仅在您批准后才会运行 { $tool }。
chat-waiting-value = 等待只有您可以输入的值。它只会发送给工具，绝不会发送给模型。
chat-waiting-answer = 等待人员的回答。
chat-composer-paused = 请先批准或拒绝上方的请求；下一条消息需等待该决定。
chat-prompt-heading = 助手提问
chat-prompt-placeholder = 输入答案…
chat-prompt-answer = 回答
chat-prompt-skip = 跳过

# 回答生成期间输入框依然可用。
chat-composer-send-during-turn-title = 发送。回答生成期间，这条会补充进去；若送达太晚，则作为下一条消息发送。
chat-composer-interrupt = 中断并重新提问
chat-composer-interrupt-title = 停止当前回答，改为发送这条。已经写出的内容仍保留在会话中。
chat-turn-waiting = 已发送 — 正在等待空闲名额
chat-turn-waiting-cancel = 撤回
chat-steer-pending = 在本条回答期间补充 — 尚未读取
chat-steer-delivered = 在本条回答期间补充 — 已采纳
chat-steer-resent = 在本条回答期间补充 — 送达太晚，已作为下一条消息发送
chat-steer-discarded = 在本条回答期间补充 — 送达太晚，已丢弃

linked-chat-label = 继续于
linked-chat-fresh = 下次运行时打开的新对话
linked-chat-help = 每次运行都会把提示和回复追加到此对话。选择你的一个对话，即可让所有内容保持在同一线程中。

# A model call that looped and was retried at a lower thinking level, shown
# collapsed above the answer. $effort is a level name (off/low/medium/high/xhigh).
chat-attempt-summary = 第 { $n } 次尝试已停止：{ $reason }（思考：{ $effort }）
chat-attempt-reason-loop = 模型开始重复自身
chat-attempt-reason-repeated_call = 反复进行同一个工具调用
chat-attempt-retry = 以思考强度 { $effort } 重新尝试（第 { $n } 次）
chat-loop-exhausted = 模型在全部 { $attempts } 次尝试中都在重复，即使降低了思考强度。请换一种方式提问，或将问题拆分成更小的部分。
