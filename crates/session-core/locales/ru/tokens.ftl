# STATUS: llm-generated, unreviewed — pending native-speaker QA

tokens-page-heading = API-токены
tokens-intro = Токены для приложений и кода, подключающихся к AIplane. Полное значение показывается только при создании или обновлении — скопируйте его и храните в секрете.

tokens-create-heading = Создать токен
tokens-name-label = Имя
tokens-name-placeholder = напр. laptop, ci-runner
tokens-ttl-label = Срок действия (дней)
tokens-create-submit = Создать токен

tokens-list-heading = Ваши токены
tokens-list-empty = Токенов пока нет. Создайте один кнопкой выше.

tokens-badge-revoked = отозван
tokens-badge-active = активен
tokens-remove-button = Удалить
tokens-rotate-button = Обновить
tokens-rotate-title = Выпустить новый секрет для этого токена (имя и настройки сохраняются)
tokens-revoke-button = Отозвать

tokens-row-meta = создан { $created } · последнее использование { $last_used } · истекает { $expires }
tokens-last-used-never = никогда

tokens-tool-use-label = Использование инструментов

tokens-mcp-allow-description = Инструменты коннектора, требующие подтверждения, не могут запрашивать его через API; включение этой опции запускает их без запроса.

tokens-minted-heading = Токен создан
tokens-minted-copy-warning = Скопируйте значение сейчас — повторно увидеть его будет нельзя.
tokens-copy-aria = Скопировать токен
tokens-minted-name = Имя: { $name }

tokens-account-user-id-label = ID пользователя

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
tokens-models-summary-restricted = Модели: выбрано { $count }
tokens-models-help = Если выключено, токен следует вашему собственному доступу, включая модели, добавленные позже. Если включено, он может использовать только отмеченные модели — добавленная после этого модель останется недоступной, пока вы не отметите её здесь.
tokens-models-restrict-label = Ограничить этот токен определёнными моделями
tokens-models-saved-toast = Токен ограничен { $count } моделями.
tokens-models-cleared-toast = Токен может использовать все ваши модели.
tokens-limits-add = Добавить квоту
tokens-limits-saved-toast = Квота токена сохранена.

# SPA-only: the Svelte /tokens row panels and their confirm prompts.
tokens-quota-max-placeholder = макс
tokens-revoke-confirm = Отозвать этот токен? Клиенты, которые его используют, сразу перестанут работать.
tokens-rotate-confirm = Выпустить новый секрет? Старый сразу перестанет работать.
tokens-remove-confirm = Удалить эту запись токена навсегда?

tokens-create-description = Создайте новый Bearer-токен для API, совместимого с OpenAI.
tokens-tool-use-description = Разрешить этому токену вызывать инструменты шлюза (веб-поиск, RAG, …).
tokens-capabilities-summary = Возможности
tokens-panel-close = Закрыть
tokens-edit-button = Изменить
tokens-mcp-allow-label = Разрешить MCP-инструменты «ask» через API
tokens-account-heading = Аккаунт
tokens-signed-in-as = Вы вошли как { $email }
tokens-account-oidc-label = Роли OIDC
tokens-account-rbac-label = ID ролей RBAC
tokens-roles-none = нет
tokens-roles-none-granted = не предоставлены
tokens-push-heading = Уведомления
tokens-push-description = Получайте уведомление на этом устройстве, когда начатый вами ответ завершится, пока вы не в приложении.
tokens-push-off = Уведомления выключены для этого устройства.
tokens-push-denied = Этот браузер заблокировал уведомления. Разрешите их в настройках браузера, чтобы включить.
tokens-push-unsupported = Этот браузер не поддерживает уведомления.
tokens-models-none-picked = Отметьте хотя бы одну модель или отключите ограничение.
tokens-limits-help = Ограничение только для этого токена. Ваш собственный бюджет продолжает действовать, поэтому это может только сузить расход токена, но не расширить его.
tokens-limits-remove = Удалить
tokens-limits-admin-badge = задано администратором
tokens-models-admin-set = Оператор дополнительно ограничивает этот токен моделями: { $models }. Ваш выбор может только сузить этот список, но не расширить.

# Client setup guides and token-page tabs.
tokens-tab-tokens = Токены
tokens-tab-guides = Инструкции
tokens-tab-account = Учётная запись
tokens-guides-heading = Настройка клиента
tokens-guides-intro = Выберите приложение ниже и следуйте шагам. Один токен можно использовать в нескольких приложениях, но отдельным токеном для каждого приложения проще управлять.
tokens-guides-before = Сначала создайте токен на вкладке «Токены» и скопируйте его. Полное значение показывается только один раз. Храните его в секрете.
tokens-guide-opencode = OpenCode
tokens-guide-claude = Claude Code
tokens-guide-python = Python (OpenAI)
tokens-opencode-step-1 = Создайте токен на вкладке «Токены» и скопируйте его.
tokens-opencode-step-2 = В OpenCode выполните /connect, выберите Other, введите aiplane как ID провайдера и вставьте токен.
tokens-opencode-step-3 = Сохраните настройки в ~/.config/opencode/opencode.jsonc. Замените YOUR_MODEL_ID на доступный ID чат-модели.
tokens-opencode-finish = Запустите opencode и выберите aiplane/YOUR_MODEL_ID через /models.
tokens-claude-step-1 = Создайте токен на вкладке «Токены» и скопируйте его.
tokens-claude-step-2 = Выберите доступный ID чат-модели. Администратор мог настроить псевдоним для Claude Code.
tokens-claude-step-3 = Вставьте эти команды в терминал. Сначала замените токен и ID модели.
tokens-claude-finish = Claude Code использует базовый URL без /v1. Если модель не найдена, уточните у администратора нужный псевдоним.
tokens-python-step-1 = Создайте токен на вкладке «Токены» и скопируйте его.
tokens-python-step-2 = Установите пакет OpenAI для Python командой python -m pip install openai, затем задайте OPENAI_API_KEY с вашим токеном в терминале.
tokens-python-step-3 = Сохраните пример как chat.py. Замените YOUR_MODEL_ID на доступный ID чат-модели.
tokens-python-finish = Запустите python chat.py. Храните токен в переменной окружения, а не в скрипте.
tokens-guide-omp = Oh My Pi
tokens-guide-pi = Pi
tokens-omp-finish = Запустите omp и выберите aiplane/YOUR_MODEL_ID командой /model.
tokens-omp-step-1 = Создайте токен на вкладке «Токены» и скопируйте его.
tokens-omp-step-2 = Сохраните эту конфигурацию в ~/.omp/agent/models.yml. Сначала замените токен и YOUR_MODEL_ID, а contextWindow укажите равным размеру контекста модели.
tokens-pi-finish = Запустите pi и выберите aiplane/YOUR_MODEL_ID командой /model.
tokens-pi-step-1 = Создайте токен на вкладке «Токены» и скопируйте его.
tokens-pi-step-2 = Сохраните эту конфигурацию в ~/.pi/agent/models.json. Сначала замените токен и YOUR_MODEL_ID.
tokens-guides-model-note = Нужен ID модели? Доступные модели есть в выборе модели чата или в GET /v1/models с вашим токеном.

notifications-loading = Загрузка настроек уведомлений…
notifications-unavailable = Уведомления недоступны на этом шлюзе. Попросите администратора проверить настройки Push.
notifications-admin-settings-link = Открыть настройки Push
tokens-tile-models = Модели
tokens-tile-tools = Инструменты
tokens-tile-budget = Бюджет
tokens-save = Сохранить
tokens-menu-aria = Действия с токеном
tokens-expires-today = истекает сегодня
tokens-expires-soon = { $days ->
    [one] истекает через { $days } день
    [few] истекает через { $days } дня
   *[other] истекает через { $days } дней
}
tokens-models-tile-all = Все модели ({ $count }) — по вашему доступу
tokens-models-tile-some = { $count } из { $total } моделей
tokens-models-tile-compliant = Все соответствуют GDPR и покрыты NDA
tokens-models-tile-noncompliant = Моделей без защиты GDPR или NDA: { $count }
tokens-models-tile-admin = Оператор разрешает из них { $count }
tokens-models-max-price = до { $price } за 1 млн выходных токенов
tokens-models-noncompliant-warning = Моделей без соответствия GDPR или без NDA: { $count }. Не отправляйте через этот токен персональные данные или конфиденциальные материалы.
tokens-models-search = Поиск моделей
tokens-models-filter-gdpr = Соответствует GDPR
tokens-models-filter-nda = Покрыто NDA
tokens-models-filter-free = Бесплатно
tokens-models-select-compliant = Выбрать все с GDPR + NDA
tokens-models-select-none = Снять выбор
tokens-models-empty = Нет подходящих моделей.
tokens-models-alias = псевдоним для { $target }
tokens-models-gdpr-ok = Соответствует GDPR: персональные данные защищены.
tokens-models-nda-ok = Покрыто соглашением о конфиденциальности.
tokens-models-price-free = бесплатно
tokens-models-price-tokens = { $input } вход / { $output } выход за 1 млн
tokens-models-price-per-images = { $price } за изображение
tokens-models-price-per-characters = { $price } за символ
tokens-models-price-per-seconds = { $price } за секунду
tokens-models-kind-chat = Чат
tokens-models-kind-transcription = Речь в текст
tokens-models-kind-speech = Текст в речь
tokens-models-kind-embedding = Эмбеддинги
tokens-models-kind-rerank = Переранжирование
tokens-models-kind-image = Изображения
tokens-models-kind-system_one = System One
tokens-tools-tile-on = Разрешены
tokens-tools-tile-off = Выключены
tokens-tools-tile-pinned = Всегда включено: { $count }
tokens-tools-tile-mcp-allowed = MCP-инструменты «Ask» запускаются без запроса
tokens-tools-tile-mcp-blocked = MCP-инструменты «Ask» заблокированы
tokens-tools-capabilities-help = Какие инструменты и навыки может использовать этот токен.
tokens-tools-saved-toast = Инструменты токена сохранены.
tokens-budget-tile-none = Собственного лимита нет
tokens-budget-tile-more = ещё { $count }
tokens-budget-owner-applies = Ваш собственный бюджет тоже действует
tokens-budget-token-heading = Этот токен
tokens-budget-owner-heading = Также действует: ваш собственный бюджет
tokens-budget-owner-none = У вас нет лимита бюджета.
tokens-budget-dimension = Что ограничить
tokens-budget-window = Период
