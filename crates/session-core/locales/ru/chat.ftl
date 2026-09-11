# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/mod.rs` — the
# multi-conversation chat page's server-side handlers: page title
# fallback, sidebar/effort/share/pin toasts, and the SSE-toast error
# messages the composer's fetch layer surfaces on failed actions.

chat-default-title = Чат

chat-error-still-streaming = Для этого пользователя ещё идёт передача ответа — подождите или нажмите «Стоп».

chat-error-auth-required = требуется авторизация
chat-error-no-such-turn = такого сообщения не существует
chat-error-db-error = ошибка базы данных
chat-error-attachments-not-configured = вложения чата не настроены
chat-error-bad-filename = недопустимое имя файла
chat-error-attachment-not-found = не найдено

# SPA-only chat chrome (`web/src/routes/chat/*`): the conversation list,
# the conversation header, the assistant's ask-back card, and the
# turn-status line the server never renders itself.
chat-list-empty = Пока нет бесед. Начните новую выше.
chat-turn-stopped = остановлено
chat-prompt-heading = Ассистент спрашивает
chat-prompt-placeholder = Введите ответ…
chat-prompt-answer = Ответить
chat-prompt-skip = Пропустить
