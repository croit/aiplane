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
chat-error-attachment-storage-failed = Хранилище вложений ответило некорректно, поэтому файл не удалось получить. Попробуйте ещё раз; если ошибка повторяется, администратору следует проверить хранилище вложений в /admin/settings.
chat-error-turn-interrupted = Внутренняя ошибка прервала этот ответ. Попробуйте ещё раз.

# SPA-only chat chrome (`web/src/routes/chat/*`): the conversation list,
# the conversation header, the assistant's ask-back card, and the
# turn-status line the server never renders itself.
chat-list-empty = Пока нет бесед. Начните новую выше.
chat-turn-stopped = остановлено
chat-waiting-approval = Запустит { $tool } только после вашего одобрения.
chat-waiting-value = Ждёт значение, которое можете ввести только вы. Оно передаётся инструменту, а не модели.
chat-waiting-answer = Ждёт ответа человека.
chat-composer-paused = Сначала одобрите или отклоните запрос выше; следующее сообщение ждёт этого решения.
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

linked-chat-label = Продолжать в
linked-chat-fresh = Новом чате, который откроет следующий запуск
linked-chat-help = Каждый запуск добавляет свой запрос и ответ в этот чат. Выберите одну из своих бесед, чтобы вести всё в одной ветке.

# A model call that looped and was retried at a lower thinking level, shown
# collapsed above the answer. $effort is a level name (off/low/medium/high/xhigh).
chat-attempt-summary = Попытка { $n } остановлена: { $reason } (размышления: { $effort })
chat-attempt-reason-loop = модель начала повторяться
chat-attempt-reason-repeated_call = один и тот же вызов инструмента снова и снова
chat-attempt-retry = Новая попытка с уровнем размышлений { $effort } (попытка { $n })
chat-loop-exhausted = Модель повторялась во всех попытках ({ $attempts }), даже с меньшим уровнем размышлений. Сформулируйте вопрос иначе или разбейте его на части.
