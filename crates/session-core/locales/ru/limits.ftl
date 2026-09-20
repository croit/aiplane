# Административный редактор лимитов запросов / квот (/admin/limits).
limits-heading = Лимиты запросов и квоты
limits-add-heading = Добавить или обновить лимит
limits-field-subject = Применяется к
limits-field-model = Модель
limits-field-dimension = Лимит
limits-field-window = За
limits-field-value = Значение
limits-add-submit = Сохранить лимит
limits-subject-global = Все (по умолчанию)
limits-subject-role = Роль
limits-subject-user = Пользователь
limits-dim-requests = Запросы
limits-dim-tokens = Токены
limits-dim-cost = Стоимость ({ $cur })
limits-dim-cost-short = Стоимость
limits-win-hour = Час
limits-win-day = День
limits-win-week = Неделя
limits-win-month = Месяц
limits-col-subject = Применяется к
limits-col-scope = Модель
limits-col-limit = Лимит
limits-col-window = Окно
limits-none = Лимиты не настроены — все безлимитны.
limits-all-models = все модели
limits-delete = Удалить
limits-edit = Изменить
limits-edit-heading = Изменить лимит
limits-edit-submit = Сохранить изменения
limits-saved = лимит сохранён для { $subject }
limits-subject-token = API-токен

# Таблица правил в SPA: заголовок, колонка «кем задано» и подтверждение удаления.
limits-delete-confirm = Удалить это правило?
limits-intro = Ограничьте, сколько запросов, токенов или средств может использовать вызывающая сторона в скользящем окне. Правила разрешаются от наиболее конкретного к общему: побеждает собственное правило пользователя, иначе — самое щедрое из его ролей, иначе — глобальное значение по умолчанию. Без правил все безлимитны. Правило для API-токена — это дополнительный потолок, который проверяется вместе с бюджетом владельца, поэтому оно может только сузить расход этого токена. Учитываются только тарифицируемые пулы (самостоятельно размещённые пулы с enforce_limits = false исключены), а весь бюджет пользователя распределяется между его API-токенами, чатом и запланированными запусками.
limits-field-subject-id = Роль / пользователь / токен
limits-field-subject-id-ph = id роли, email пользователя или id токена
limits-col-value = Значение
limits-col-actions = Действия
limits-deleted = лимит удалён
