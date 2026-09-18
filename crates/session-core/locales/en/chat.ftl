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

# SPA-only chat chrome (`web/src/routes/chat/*`): the conversation list,
# the conversation header, the assistant's ask-back card, and the
# turn-status line the server never renders itself.
chat-list-empty = No conversations yet. Start one above.
chat-turn-stopped = stopped
chat-prompt-heading = The assistant asks
chat-prompt-placeholder = Type an answer…
chat-prompt-answer = Answer
chat-prompt-skip = Skip

# The composer stays usable while a turn runs: what was typed waits in a
# queue, an interjection can be thrown into the running answer, and the
# answer can be cut short to re-aim it.
chat-queue-label = { $count ->
    [one] { $count } message waiting
   *[other] { $count } messages waiting
  }
chat-queue-move-up = Move up
chat-queue-move-down = Move down
chat-queue-edit = Put back in the composer
chat-queue-remove = Discard
chat-queue-held-title = The attachment was lost when the page reloaded — re-attach it or discard this message
chat-queue-held-hint = A waiting message lost its attachment when the page reloaded. It is not sent until you put it back in the composer and attach the file again.
chat-queue-was-interjection = Was typed during the previous answer, which ended before reading it
chat-composer-interject = Add to the running answer
chat-composer-interject-title = Hand this to the answer being written (Ctrl/Cmd+Enter). It arrives at the next tool step; if the answer ends first, it is sent as the next message.
chat-composer-interrupt = Interrupt and re-aim
chat-composer-interrupt-title = Stop the current answer and send this instead. What was written so far stays in the conversation.
chat-steer-pending = Added during this answer — not read yet
chat-steer-delivered = Added during this answer — taken into account
chat-steer-resent = Added during this answer — arrived too late, sent as the next message
chat-steer-discarded = Added during this answer — arrived too late and was discarded
