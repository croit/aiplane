# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/mod.rs` — the
# multi-conversation chat page's server-side handlers: page title
# fallback, sidebar/effort/share/pin toasts, and the SSE-toast error
# messages the composer's fetch layer surfaces on failed actions.

chat-default-title = Чат

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

# Поле ввода остаётся доступным, пока пишется ответ.
chat-composer-send-during-turn-title = Отправить. Пока пишется ответ, это добавляется к нему; если не успеет — будет отправлено следующим сообщением.
chat-composer-interrupt = Прервать и перенаправить
chat-composer-interrupt-title = Остановить текущий ответ и отправить вместо него это. Написанное остаётся в переписке.
chat-turn-waiting = Отправлено — ждёт свободного слота
chat-turn-waiting-cancel = Отозвать
chat-steer-pending = Добавлено во время этого ответа — ещё не прочитано
chat-steer-delivered = Добавлено во время этого ответа — учтено
chat-steer-resent = Добавлено во время этого ответа — пришло поздно, отправлено следующим сообщением
chat-steer-discarded = Добавлено во время этого ответа — пришло поздно и было отброшено
