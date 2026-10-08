# Strings owned by `gateway/src/rama_server/pages/chat/mod.rs` — the
# multi-conversation chat page's server-side handlers: page title
# fallback, sidebar/effort/share/pin toasts, and the SSE-toast error
# messages the composer's fetch layer surfaces on failed actions.

chat-default-title = Chat

chat-error-auth-required = auth required
chat-error-no-such-turn = no such turn
chat-error-db-error = db error
chat-error-attachments-not-configured = chat attachments not configured
chat-error-bad-filename = bad filename
chat-error-attachment-not-found = not found
chat-error-attachment-storage-failed = The attachment store did not answer properly, so the file could not be fetched. Try again; if it keeps failing, an administrator should check the attachment storage under /admin/settings.
chat-error-turn-interrupted = An internal error interrupted this response. Please try again.

# SPA-only chat chrome (`web/src/routes/chat/*`): the conversation list,
# the conversation header, the assistant's ask-back card, and the
# turn-status line the server never renders itself.
chat-list-empty = No conversations yet. Start one above.
chat-turn-stopped = stopped
chat-waiting-approval = Runs { $tool } only once you approve it.
chat-waiting-value = Waits for a value only you may type. It goes to the tool, never to the model.
chat-waiting-answer = Waits for a person's answer.
chat-composer-paused = Approve or deny the request above first; the next message waits for that decision.
chat-prompt-heading = The assistant asks
chat-prompt-placeholder = Type an answer…
chat-prompt-answer = Answer
chat-prompt-skip = Skip

# The composer stays usable while a turn runs: what was typed waits in a
# queue, an interjection can be thrown into the running answer, and the
# answer can be cut short to re-aim it.
chat-composer-send-during-turn-title = Send. While an answer is being written this is added to it, and if it arrives too late it is sent as the next message.
chat-composer-interrupt = Interrupt and re-aim
chat-composer-interrupt-title = Stop the current answer and send this instead. What was written so far stays in the conversation.
chat-turn-waiting = Sent — waiting for a free slot
chat-turn-waiting-cancel = Take back
chat-steer-pending = Added during this answer — not read yet
chat-steer-delivered = Added during this answer — taken into account
chat-steer-resent = Added during this answer — arrived too late, sent as the next message
chat-steer-discarded = Added during this answer — arrived too late and was discarded

linked-chat-label = Continue in
linked-chat-fresh = A new chat, opened by the next run
linked-chat-help = Every run adds its prompt and reply to this chat. Pick one of your conversations to keep a single thread.

# A model call that looped and was retried at a lower thinking level, shown
# collapsed above the answer. $effort is a level name (off/low/medium/high/xhigh).
chat-attempt-summary = Attempt { $n } stopped: { $reason } (thinking: { $effort })
chat-attempt-reason-loop = the model repeated itself
chat-attempt-reason-repeated_call = the same tool call over and over
chat-attempt-retry = Trying again with thinking { $effort } (attempt { $n })
chat-loop-exhausted = The model repeated itself on all { $attempts } attempts, even with less thinking. Ask the question differently, or split it into smaller parts.
