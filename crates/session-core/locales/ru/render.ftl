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
render-composer-send = Отправить
render-composer-stop = Стоп

# The browser prompt behind the ✎ Edit button, and the per-file title on
# an attachment's remove button.
render-edit-prompt = Измените своё сообщение:
render-attachment-remove-title = Удалить { $filename }
