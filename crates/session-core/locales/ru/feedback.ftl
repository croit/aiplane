# Виджет обратной связи: плавающая кнопка, диалог (форма, голосовой ввод,
# аннотирование снимка экрана, вложения, согласие на диагностику),
# подтверждение публичного трекера и ошибки `/api/v0/feedback*`.

feedback-fab-aria = Отправить отзыв
feedback-fab-title = Отправить отзыв

feedback-dialog-heading = Отправить отзыв
feedback-dialog-blurb = Страница, ваш браузер и недавняя активность консоли и сети прикрепляются автоматически.
feedback-close-aria = Закрыть
feedback-cancel-button = Отмена
feedback-submit-button = Отправить отзыв
feedback-sending = Отправка…

feedback-title-label = Заголовок
feedback-title-placeholder = Краткая сводка
feedback-description-label = Описание
feedback-description-placeholder = Что произошло или чего вам хотелось бы?
feedback-business-label = Польза для бизнеса
feedback-business-placeholder = Почему это важно? Кого это затрагивает?
feedback-acceptance-label = Критерии приёмки
feedback-acceptance-placeholder = Когда задача считается выполненной?
feedback-priority-label = Приоритет
feedback-priority-low = Низкий
feedback-priority-medium = Средний
feedback-priority-high = Высокий

# Голосовой ввод — одна запись заполняет все поля выше.
feedback-voice-button-label = Заполнить голосом
feedback-voice-button-title = Нажмите, опишите проблему, нажмите ещё раз — мы заполним поля ниже
feedback-voice-stop-label = Остановить и заполнить
feedback-voice-working-label = Распознавание…
feedback-voice-applied = Заполнено по вашей записи — проверьте и отправьте.
feedback-voice-no-speech = Речь не распознана — попробуйте ещё раз.

# Снимок экрана и аннотации.
feedback-shot-label = Снимок экрана
feedback-shot-status-capturing = Съёмка…
feedback-shot-status-attached = Прикреплён — рисуйте поверх, чтобы добавить пометки
feedback-shot-status-none = Снимка экрана нет
feedback-shot-status-failed = Снимок экрана недоступен
feedback-shot-capture = Добавить снимок экрана
feedback-shot-recapture = Снять заново
feedback-shot-remove = Удалить
feedback-shot-exact = Точный по пикселям
feedback-shot-exact-title = Снимает реальные пиксели экрана через диалог демонстрации экрана браузера — включая canvas, WebGL и встроенные фреймы, которые обычный снимок воспроизвести не может.
feedback-shot-exact-cancelled = Демонстрация экрана отменена — текущий снимок сохранён.
feedback-shot-capture-failed = Не удалось сделать снимок экрана.

feedback-shot-annotate = Пометить
feedback-annotate-heading = Пометки на снимке экрана
feedback-annotate-done = Назад к форме
feedback-annotate-hint = Тяните, чтобы рисовать. Инструментом перемещения (или средней кнопкой мыши) двигайте увеличенное изображение; ctrl/⌘ + колесо — масштаб.
feedback-tool-pan-title = Перемещение
feedback-zoom-preset-title = Нажмите, чтобы вписать весь снимок; нажмите ещё раз — по ширине
feedback-zoom-fit-label = Вписать
feedback-zoom-width-label = По ширине

feedback-tool-rect-title = Прямоугольник
feedback-tool-arrow-title = Стрелка
feedback-tool-pen-title = Свободное рисование
feedback-tool-text-title = Текст
feedback-tool-redact-title = Скрыть / закрасить (залитый прямоугольник)
feedback-tool-text-prompt = Текст пометки
feedback-color-aria = Цвет
feedback-undo-title = Отменить
feedback-redo-title = Вернуть
feedback-clear-annot-title = Очистить пометки
feedback-clear-annot-label = Очистить
feedback-zoom-out-title = Уменьшить
feedback-zoom-in-title = Увеличить

# Дополнительные изображения, вставленные или перетащенные на форму.
feedback-attachments-label = Изображения
feedback-attachments-count = { $count } из { $max }
feedback-attachments-hint = Вставьте или перетащите сюда изображения, чтобы прикрепить их.
feedback-attachments-drop = Отпустите, чтобы прикрепить
feedback-attachments-remove = Удалить изображение
feedback-attachments-too-many = Не более { $max } изображений.
feedback-attachments-too-large = Это изображение больше { $max } МБ.
feedback-attachments-invalid = Прикреплять можно только файлы изображений.

# Согласие на диагностику. Оба пункта включены по умолчанию; журнал чата
# появляется только на странице разговора.
feedback-log-browser-label = Отправить журнал активности браузера (консоль + сеть)
feedback-log-chat-label = Отправить журнал чата и инструментов
feedback-diagnostics-label = Показать данные, которые будут прикреплены

feedback-confirm-heading = Вы уверены?
feedback-confirm-public-p1-prefix = Этот отзыв создаёт задачу в нашем
feedback-confirm-public-p1-strong = публичном
feedback-confirm-public-p1-suffix = трекере задач. Её сможет прочитать кто угодно.
feedback-confirm-private-p2-prefix = Убедитесь, что снимок экрана и отправляемые данные не содержат
feedback-confirm-private-p2-strong = никакой персональной или приватной информации
feedback-confirm-private-p2-suffix = (имён, адресов почты, токенов, клиентских данных …).
feedback-confirm-cancel-button = Нет, хочу отредактировать
feedback-confirm-ok-button = Да, отправить

feedback-thanks-heading = Спасибо
feedback-thanks-body = Ваш отзыв создан как задача.
feedback-thanks-issue = Создана задача №{ $number }.
feedback-thanks-open = Открыть задачу
feedback-done-button = Готово

feedback-err-no-session = Нет активной сессии
feedback-err-session-lookup-failed = Не удалось найти сессию
feedback-err-body-read = { $error }
feedback-err-empty-transcript = Пустая расшифровка
feedback-err-malformed-json = Некорректный JSON: { $error }
feedback-err-no-chat-model = Нет доступной чат-модели для разбора
feedback-err-extraction-failed = Разбор не удался: { $error }
feedback-err-not-configured = Обратная связь не настроена
feedback-err-title-required = Требуется заголовок (не менее 4 символов)
feedback-err-description-required = Требуется описание
feedback-err-submit-failed = Не удалось создать задачу — попробуйте ещё раз
feedback-err-network = Сетевая ошибка: { $error }
