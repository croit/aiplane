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
chat-queue-label = { $count ->
    [one] { $count } сообщение в очереди
   *[other] { $count } сообщений в очереди
  }
chat-queue-move-up = Выше
chat-queue-move-down = Ниже
chat-queue-edit = Вернуть в поле ввода
chat-queue-remove = Отбросить
chat-queue-held-title = Вложение потерялось при перезагрузке страницы — прикрепите его снова или отбросьте сообщение
chat-queue-held-hint = Сообщение в очереди потеряло вложение при перезагрузке страницы. Оно не будет отправлено, пока вы не вернёте его в поле ввода и не прикрепите файл заново.
chat-queue-was-interjection = Написано во время предыдущего ответа, который закончился раньше
chat-composer-interject = Добавить к текущему ответу
chat-composer-interject-title = Передаётся ответу, который сейчас пишется (Ctrl/Cmd+Ввод). Дойдёт на следующем шаге с инструментом; если ответ закончится раньше, будет отправлено следующим сообщением.
chat-composer-interrupt = Прервать и перенаправить
chat-composer-interrupt-title = Остановить текущий ответ и отправить вместо него это. Написанное остаётся в переписке.
chat-steer-pending = Добавлено во время этого ответа — ещё не прочитано
chat-steer-delivered = Добавлено во время этого ответа — учтено
chat-steer-resent = Добавлено во время этого ответа — пришло поздно, отправлено следующим сообщением
chat-steer-discarded = Добавлено во время этого ответа — пришло поздно и было отброшено
