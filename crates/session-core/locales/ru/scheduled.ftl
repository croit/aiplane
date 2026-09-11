# STATUS: llm-generated, unreviewed — pending native-speaker QA


scheduled-heading = Запланированные действия
scheduled-intro = Запускайте промпт автоматически по расписанию. Каждый запуск открывает новый чат, который можно прочитать здесь — выберите модель, напишите промпт и укажите, когда он должен запускаться.
scheduled-create-submit = Создать запланированное действие
scheduled-list-heading = Ваши запланированные действия
scheduled-list-empty = Пока нет запланированных действий. Создайте одно выше.

scheduled-back = Назад
scheduled-edit-heading = Изменить запланированное действие
scheduled-save-submit = Сохранить изменения

scheduled-name-label = Название
scheduled-name-placeholder = напр. Ежедневная сводка новостей
scheduled-model-label = Модель
scheduled-model-placeholder = идентификатор модели (напр. gpt-4o-mini)
scheduled-gdpr-warning = Эта модель не соответствует требованиям GDPR. Запланированные запуски будут автоматически отправлять ей ваш промпт — избегайте персональных данных.
scheduled-nda-warning = Эта модель не защищена соглашением о конфиденциальности. Не планируйте отправку материалов, защищённых NDA или являющихся собственностью компании, в эту модель.
scheduled-prompt-label = Промпт
scheduled-prompt-placeholder = Что модель должна делать при каждом запуске?
scheduled-tools-toggle-label = Разрешить инструменты (веб-поиск, RAG, вложения) — как в чате
scheduled-reuse-toggle-label = Использовать чат предыдущего запуска повторно — каждый запуск продолжает тот же разговор
scheduled-reuse-rounds-prefix = отправлять последние
scheduled-reuse-rounds-aria = Количество раундов истории для повторного воспроизведения
scheduled-reuse-rounds-suffix = раундов

scheduled-builder-heading = Расписание
scheduled-mode-hourly = Ежечасно
scheduled-mode-daily = Ежедневно
scheduled-mode-weekly = Еженедельно
scheduled-mode-monthly = Ежемесячно
scheduled-mode-advanced = Расширенно
scheduled-weekday-mon = Пн
scheduled-weekday-tue = Вт
scheduled-weekday-wed = Ср
scheduled-weekday-thu = Чт
scheduled-weekday-fri = Пт
scheduled-weekday-sat = Сб
scheduled-weekday-sun = Вс
scheduled-on-day-label = В день
scheduled-of-every-month = каждого месяца
scheduled-at-label = В
scheduled-hour-aria = Час
scheduled-minute-aria = Минута
scheduled-of-every-hour = каждого часа
scheduled-timezone-label = Часовой пояс
scheduled-timezone-placeholder = Europe/Berlin
scheduled-cron-label = Cron-выражение
scheduled-cron-help = Пять полей: минута час день-месяца месяц день-недели.

scheduled-no-upcoming-runs = Нет предстоящих запусков.
scheduled-next-runs-prefix = Следующие запуски:{ " " }

scheduled-err-pick-weekday = Выберите хотя бы один день недели.
scheduled-err-enter-cron = Введите cron-выражение.

scheduled-model-non-gdpr = { $model } (не соответствует GDPR)
scheduled-model-nda-restricted = { $model } (ограничение конфиденциальности)
scheduled-model-non-gdpr-nda-restricted = { $model } (не соответствует GDPR, ограничение конфиденциальности)

scheduled-toast-not-found = Такого запланированного действия не существует.

scheduled-badge-active = активно
scheduled-badge-paused = приостановлено
scheduled-status-paused = Приостановлено
scheduled-next-run = Следующий запуск: { $when }
scheduled-no-upcoming-run = Нет предстоящих запусков
scheduled-last-success = Последний: ✓ { $when }
scheduled-last-success-open = Последний: ✓ { $when } — открыть
scheduled-last-failure = Последний: ✗ { $when }
scheduled-last-failure-open = Последний: ✗ { $when } — открыть
scheduled-pause-title = Приостановить
scheduled-resume-title = Возобновить
scheduled-edit-title = Изменить
scheduled-delete-title = Удалить
scheduled-delete-confirm = Удалить это запланированное действие?
scheduled-preview-summary = { $summary } ({ $timezone })
scheduled-preview-next-runs = Следующие запуски: { $runs }
scheduled-last-error = Последняя ошибка: { $error }
