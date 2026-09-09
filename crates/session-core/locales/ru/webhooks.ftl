# STATUS: llm-generated, unreviewed — pending native-speaker QA

webhooks-heading = Вебхуки
webhooks-intro = Запускайте промпт, когда внешний сервис обращается к URL. Вы получаете секретный URL-триггер; то, что вызывающая сторона отправляет в теле запроса, добавляется к вашему промпту, а выполнение открывается как новый чат, который можно прочитать здесь.
webhooks-edit-heading = Редактировать вебхук
webhooks-list-empty = Пока нет вебхуков. Создайте один выше.

webhooks-name-label = Название
webhooks-name-placeholder = напр. Сводка развёртывания
webhooks-model-label = Модель
webhooks-model-placeholder = ID модели
webhooks-prompt-placeholder = Что модель должна сделать с входящими данными?

webhooks-reveal-heading = Ваш URL-триггер
webhooks-reveal-note = Скопируйте сейчас — он показывается только один раз. Любой, у кого есть этот URL, может запустить вебхук. Потеряли? Смените секрет, чтобы получить новый.
webhooks-copy = Копировать

webhooks-badge-active = Активен
webhooks-badge-paused = Приостановлен
webhooks-mode-sync = Ждёт ответа

webhooks-pause-title = Приостановить
webhooks-resume-title = Возобновить
webhooks-rotate-title = Сменить секрет
webhooks-edit-title = Редактировать
webhooks-delete-title = Удалить

# --- Перезапуск с другим промптом ---
webhooks-toast-rerun-started = Перезапуск завершён — открываю беседу…

# --- История запусков ---
webhooks-runs-empty = Пока нет запусков. Запустите вебхук, чтобы увидеть историю здесь.
webhooks-run-open = открыть чат
webhooks-run-rerun = перезапустить

# SPA-only: the Svelte /webhooks page — inline form, run list, rerun composer.
webhooks-new-heading = Новый вебхук
webhooks-prompt-untrusted-label = Промпт (полезная нагрузка поступает как недоверенный ввод)
webhooks-runs-show = Запуски
webhooks-runs-hide = Скрыть запуски
webhooks-rerun-prompt-label = Промпт для повторного запуска — сохранённая нагрузка воспроизводится через него
webhooks-rerun-latest = Повторить последнюю нагрузку
webhooks-rerun-running = Выполняется…
webhooks-toast-rerun-failed = Повторный запуск: { $status }
webhooks-rotate-confirm = Выпустить новый секрет триггера? Старый URL сразу перестанет работать.
webhooks-delete-confirm = Удалить этот вебхук?
