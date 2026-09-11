# STATUS: llm-generated, unreviewed — pending native-speaker QA


webhooks-heading = Вебхуки
webhooks-intro = Запускайте промпт, когда внешний сервис обращается к URL. Вы получаете секретный URL-триггер; то, что вызывающая сторона отправляет в теле запроса, добавляется к вашему промпту, а выполнение открывается как новый чат, который можно прочитать здесь.
webhooks-create-submit = Создать вебхук
webhooks-save-submit = Сохранить изменения
webhooks-edit-heading = Редактировать вебхук
webhooks-back = Назад
webhooks-list-heading = Ваши вебхуки
webhooks-list-empty = Пока нет вебхуков. Создайте один выше.

webhooks-name-label = Название
webhooks-name-placeholder = напр. Сводка развёртывания
webhooks-model-label = Модель
webhooks-model-placeholder = ID модели
webhooks-prompt-label = Промпт
webhooks-prompt-placeholder = Что модель должна сделать с входящими данными?

webhooks-sync-toggle-label = Дождаться ответа (вернуть вывод модели вызывающей стороне)
webhooks-tools-toggle-label = Разрешить инструменты (запуск с вашими инструментами, напр. веб-поиск, RAG, коннекторы)
webhooks-tools-warning = Любой, у кого есть URL-триггер, может отправить контент, который модель обработает с вашими инструментами от вашего имени. Включайте это только для доверенной вызывающей стороны.

webhooks-gdpr-warning = Эта модель работает за пределами ЕС. Не отправляйте персональные данные через этот вебхук.
webhooks-nda-warning = Эта модель не допущена к контенту под NDA. Не отправляйте конфиденциальные данные через этот вебхук.
webhooks-model-non-gdpr = { $model } (вне ЕС)
webhooks-model-nda-restricted = { $model } (ограничение NDA)
webhooks-model-non-gdpr-nda-restricted = { $model } (вне ЕС, ограничение NDA)

webhooks-reveal-heading = Ваш URL-триггер
webhooks-reveal-note = Скопируйте сейчас — он показывается только один раз. Любой, у кого есть этот URL, может запустить вебхук. Потеряли? Смените секрет, чтобы получить новый.
webhooks-copy = Копировать

webhooks-badge-active = Активен
webhooks-badge-paused = Приостановлен
webhooks-mode-sync = Ждёт ответа
webhooks-mode-async = Без ожидания
webhooks-never-fired = Ещё не запускался
webhooks-last-success = Последний запуск { $when }
webhooks-last-success-open = Последний запуск { $when } — открыть
webhooks-last-failure = Последний запуск не удался { $when }
webhooks-last-failure-open = Последний запуск не удался { $when } — открыть

webhooks-pause-title = Приостановить
webhooks-resume-title = Возобновить
webhooks-rotate-title = Сменить секрет
webhooks-edit-title = Редактировать
webhooks-delete-title = Удалить


webhooks-toast-not-found = Вебхук не найден.

# --- Перезапуск с другим промптом ---
webhooks-rerun-link = перезапустить
webhooks-rerun-heading = Перезапустить с другим промптом
webhooks-rerun-page-name = Перезапуск вебхука
webhooks-rerun-intro = Повторно обработайте последнюю полезную нагрузку, полученную этим вебхуком, с промптом, который можно отредактировать. Запуск откроется как новый чат.
webhooks-rerun-payload-label = Захваченная полезная нагрузка (воспроизводится как есть)
webhooks-rerun-submit = Перезапустить
webhooks-rerun-no-payload = Этот вебхук ещё не захватил полезную нагрузку — сначала запустите его один раз.
webhooks-rerun-no-payload-notice = Этот вебхук ещё не запускался, поэтому нет полезной нагрузки для воспроизведения. Запустите его один раз, затем вернитесь, чтобы перезапустить с другим промптом.

# --- История запусков ---
webhooks-runs-link = запуски
webhooks-runs-heading = Запуски · { $name }
webhooks-runs-page-name = Запуски вебхука
webhooks-runs-intro = Последние срабатывания и перезапуски. Откройте запуск, чтобы прочитать его беседу, или перезапустите его полезную нагрузку с другим промптом.
webhooks-runs-empty = Пока нет запусков. Запустите вебхук, чтобы увидеть историю здесь.
webhooks-run-open = открыть чат
webhooks-run-rerun = перезапустить
webhooks-run-source-fire = срабатывание
webhooks-run-source-rerun = перезапуск
webhooks-run-status-ok = ок
webhooks-run-status-error = ошибка
webhooks-run-status-pending = выполняется

# --- Повторное использование беседы ---
webhooks-reuse-toggle-label = Повторно использовать беседу (каждое срабатывание продолжает предыдущий чат)
webhooks-reuse-rounds-prefix = воспроизводя последние
webhooks-reuse-rounds-suffix = раундов
webhooks-reuse-rounds-aria = Сколько раундов истории воспроизводить

webhooks-toast-rerun-failed = Повторный запуск: { $status }
webhooks-rotate-confirm = Выпустить новый секрет триггера? Старый URL сразу перестанет работать.
webhooks-delete-confirm = Удалить этот вебхук?
