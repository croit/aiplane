# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `session-core/src/render.rs` — the HTML renderers for
# the chat-style session UI (conversation bubbles, tool-call rows, the
# document canvas, and the composer). Driver-agnostic: both the gateway
# and any future consumer of this crate render through these functions.

render-edit-button = ✎ Изменить

render-retry-button = ↻ Повторить

render-attachment-remove-aria = Удалить вложение

# Кнопка копирования у блока кода в ответе (только значок, поэтому это
# подсказка / доступное имя).

render-thinking-spinner = Думает…
render-thinking-finalized = Думал { $secs } с

render-tool-status-used = Использован

render-canvas-edit-button = ✎ Редактировать
render-canvas-save = Сохранить как новую версию
render-canvas-cancel = Отмена

render-composer-attach-aria = Прикрепить файлы
render-composer-attach-title = Прикрепить файлы (также перетаскиванием / вставкой)
render-composer-record-aria = Записать голосовое сообщение
render-composer-record-title = Запись
render-composer-send = Отправить
render-composer-stop = Стоп

# The browser prompt behind the ✎ Edit button, and the per-file title on
# an attachment's remove button.
render-edit-prompt = Измените своё сообщение:
render-attachment-remove-title = Удалить { $filename }
render-edit-confirm = Сохранить и сгенерировать заново? Это удалит все сообщения ниже.
render-edit-save = Сохранить и сгенерировать заново
render-edit-cancel = Отмена
render-retry-confirm = Сгенерировать этот ответ заново? Это удалит его и всё, что ниже.
render-attachment-remove-confirm = Удалить { $filename }? Это действие нельзя отменить.
render-attachment-unavailable-title = Этот вложенный файл больше недоступен
render-attachment-unavailable-meta = недоступно
render-attachment-open-title = Открыть { $filename } · { $mime } · { $size }
render-attachment-title = { $filename } · { $mime } · { $size }
render-media-label = { $kind ->
    [image] Изображение { $n }
    [video] Видео { $n }
    [audio] Аудио { $n }
   *[other] Медиа { $n }
}
render-code-copy = Копировать код
render-code-copied = Скопировано
render-still-working-spinner = Ещё работает…
render-thinking-in-progress = Размышляет… ({ $secs } с)
render-tools-running = Инструменты выполняются
render-tools-errored = Вызовы инструментов
render-tools-used = Использованные инструменты
render-tools-summary = { $count } вызовов · { $breakdown }
render-tool-status-calling = Вызывается
render-tool-status-error = Ошибка инструмента
render-tool-input-label = Входные данные
render-tool-output-label = Результат
render-tool-output-truncated = усечено для отображения — все { $bytes } байт по-прежнему доступны модели и сохранены в базе данных; отображаются первые { $chars } симв.
render-compaction-divider = Ранние сообщения свёрнуты для экономии контекста
render-canvas-version-by-you = вы
render-canvas-hand-edited = изменено вами
render-canvas-edit-hint = Сохраняется как новая версия; ассистент узнает о вашей правке.
render-canvas-version-aria = Версия
render-canvas-resize-aria = Изменить размер холста
