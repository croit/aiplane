# Strings owned by `gateway/src/rama_server/pages/chat/mod.rs` — the
# multi-conversation chat page's server-side handlers: page title
# fallback, sidebar/effort/share/pin toasts, and the SSE-toast error
# messages the composer's fetch layer surfaces on failed actions.

chat-default-title = Chat

chat-error-still-streaming = A response is still streaming for this user — wait for it or hit stop.

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
chat-list-pinned-badge = pinned
chat-list-pin = Pin
chat-list-unpin = Unpin
chat-list-delete = Delete
chat-push-invite = Get notified when a reply finishes.
chat-all-chats = All chats
chat-turn-stopped = stopped
chat-prompt-heading = The assistant asks
chat-prompt-placeholder = Type an answer…
chat-prompt-answer = Answer
chat-prompt-skip = Skip
