# STATUS: llm-generated, unreviewed — pending native-speaker QA

tokens-page-heading = API-токены
tokens-intro = Bearer-токены для API, совместимого с OpenAI. Открытый текст показывается только при создании — сохраните его в надёжном месте.

tokens-create-heading = Создать токен
tokens-name-label = Имя
tokens-name-placeholder = напр. laptop, ci-runner
tokens-ttl-label = Срок действия (дней)
tokens-create-submit = Создать токен

tokens-list-heading = Ваши токены
tokens-list-empty = Токенов пока нет. Создайте один выше.

tokens-badge-revoked = отозван
tokens-badge-active = активен
tokens-remove-button = Удалить
tokens-rotate-button = Обновить
tokens-rotate-title = Выпустить новый секрет для этого токена (имя и настройки сохраняются)
tokens-revoke-button = Отозвать

tokens-row-meta = создан { $created } · последнее использование { $last_used } · истекает { $expires }
tokens-last-used-never = никогда

tokens-tool-use-aria = Использование инструментов
tokens-tool-use-label = Использование инструментов

tokens-mcp-allow-description = Инструменты коннектора, требующие подтверждения, не могут запрашивать его через API; включение этой опции запускает их без запроса.

tokens-minted-heading = Токен создан
tokens-minted-copy-warning = Скопируйте значение сейчас — повторно увидеть его будет нельзя.
tokens-copy-aria = Скопировать токен
tokens-copy-title = Скопировать токен
tokens-minted-name = Имя: { $name }

tokens-account-user-id-label = ID пользователя

tokens-mcp-ask-enabled-toast = MCP-инструменты «ask» через API включены для этого токена.
tokens-mcp-ask-disabled-toast = MCP-инструменты «ask» через API отключены для этого токена.

# Web Push "turn complete" opt-in card (rendered by `render_push_card`; wired
# client-side by `ui/ts/push.ts`). Device-local notification settings.
tokens-push-enable = Включить на этом устройстве
tokens-push-disable = Выключить на этом устройстве
tokens-push-on = Уведомления включены для этого устройства.
tokens-push-enabled = Уведомления включены на этом устройстве.
tokens-push-disabled = Уведомления выключены на этом устройстве.
tokens-push-error = Не удалось изменить настройки уведомлений.

# Использование, список разрешённых моделей и квота для токена (/tokens).
tokens-usage-line = в этом месяце: { $requests } запросов · { $tokens } токенов · { $cost }
tokens-models-summary-all = Модели: все
tokens-models-summary-restricted = Модели: выбрано { $count }
tokens-models-help = Если выключено, токен следует вашему собственному доступу, включая модели, добавленные позже. Если включено, он может использовать только отмеченные модели — добавленная после этого модель останется недоступной, пока вы не отметите её здесь.
tokens-models-restrict-label = Ограничить этот токен определёнными моделями
tokens-models-save = Сохранить модели
tokens-models-saved-toast = Токен ограничен { $count } моделями.
tokens-models-cleared-toast = Токен может использовать все ваши модели.
tokens-limits-add = Добавить квоту
tokens-limits-saved-toast = Квота токена сохранена.

# SPA-only: the Svelte /tokens row panels and their confirm prompts.
tokens-models-heading = Список разрешённых моделей
tokens-models-input-placeholder = идентификаторы моделей через запятую
tokens-quota-heading = Квота
tokens-quota-per = за
tokens-quota-max-placeholder = макс
tokens-mcp-heading = MCP-коннекторы
tokens-mcp-allow-button = Разрешить выполнение
tokens-mcp-block-button = Блокировать
tokens-revoke-confirm = Отозвать этот токен? Клиенты, которые его используют, сразу перестанут работать.
tokens-rotate-confirm = Выпустить новый секрет? Старый сразу перестанет работать.
tokens-remove-confirm = Удалить эту запись токена навсегда?
