# STATUS: llm-generated, unreviewed — pending native-speaker QA
# The upstream board and the backend editor: the pool band's status word, the
# editor's fields, and the connection-test outcomes.
backends-status-up = активен


# Backend CRUD editor (add/edit/delete backends stored in the DB topology).
backends-field-name = Название
backends-field-base-url = Базовый URL
backends-field-pool = Пул
backends-field-pool-none = (нет)
backends-field-pools = Пулы
backends-field-pools-hint = По одному пулу на каждый тип моделей, которые обслуживает аккаунт. Тип присоединяется к существующему пустому пулу этого типа, получает новый пул в том же сохранении или не обслуживается. Пулы, которые уже содержат другой бэкенд, не предлагаются — смешение зафиксировало бы этот бэкенд на этом списке.
backends-pool-not-served = Не обслуживается
backends-pool-new = Новый: openrouter-{ $kind }
backends-save-backend = Сохранить бэкенд
backends-delete-backend = Удалить
backends-delete-confirm = Удалить бэкенд «{ $name }»? Остальные бэкенды пула сохранятся.
backends-advanced = Дополнительные настройки

backends-field-api-key = Ключ API
backends-field-api-key-keep = оставьте пустым, чтобы сохранить текущий ключ
backends-field-api-key-stored = Сохранённый ключ: { $hint }

# An alias that is configured but would not route.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.

# Строки бэкендов в SPA: переключатель обслуживания, нагрузка за час и удаление.
backends-add-heading = Добавить бэкенд
backends-name-taken = Бэкенд с таким именем уже существует — сохранение перезапишет его вместе с базовым URL, ключом, моделями и пулом. Выберите другое имя, чтобы добавить второй бэкенд.
backends-field-api-key-placeholder = Ключ API (хранится в зашифрованном виде)
backends-field-api-key-env = Переменная окружения с API-ключом
backends-field-health-path = Путь проверки состояния
backends-field-pool-hint = Назначает этот бэкенд одному пулу. Бэкенд в нескольких пулах сводится к выбранному здесь.
backends-field-weight = Вес
backends-field-max-inflight = Макс. одновременных
backends-field-models = Модели (через запятую; пусто = определять через /models)
backends-field-aliases = Алиасы (name=target по одному в строке)
backends-field-extra-headers = Дополнительные заголовки (имя: значение на строку)
backends-field-extra-headers-hint = Отправляется с каждым запросом к этому бэкенду, после API-ключа. Например anthropic-version: 2023-06-01, который требует Anthropic.
backends-field-supports-edit = Поддерживает редактирование изображений
backends-no-models-title = Этот бэкенд не объявляет ни одной модели, поэтому на него ничего не маршрутизируется, а голому псевдониму не к чему привязаться. Обычно это проба, которая ни разу не вернула данные.
backends-no-models = нет моделей
backends-key-env-badge = ключ: env { $var }
backends-key-env-unset-badge = env { $var } НЕ ЗАДАНА
backends-enabled-hint = Выключите, чтобы вывести бэкенд на обслуживание. Действует сразу. Его модели остаются известными, поэтому нагрузку берут другие бэкенды пула, а клиенты видят временную недоступность, а не «модель не найдена».
backends-enabled-label = Принимает трафик
backends-activity-summary = 15м { $m15 } · 30м { $m30 } · 60м { $m60 }
backends-alias-target-title = алиас → { $target }
backends-alias-missing-title = алиас → { $target } — этот бэкенд не сообщает такую модель
backends-alias-disabled-title = простой алиас отключён — этот бэкенд обслуживает несколько моделей; укажите явную цель (форма сопоставления)
backends-alias-disabled-label = { $name } (отключён)
backends-fallback-offline-title = fallback_offline: используется, когда все бэкенды известной модели в этом пуле недоступны
backends-fallback-offline-badge = офлайн ↩ { $model }
backends-pool-empty = В этом пуле нет бэкендов.

backends-test-button = Проверить соединение
backends-test-hint = Обращается к этому URL с указанными выше учётными данными. Ничего не сохраняется.
backends-test-insert-hint = Объявленные id моделей — нажмите, чтобы дополнить строку псевдонима под курсором:
backends-test-ok = Доступен, аутентификация пройдена ({ $source }), найдено моделей: { $count }.
backends-test-ok-no-models = Доступен, аутентификация пройдена ({ $source }), но ответ не является списком /models ни в формате OpenAI, ни в формате TypeSafe — обнаружение не может его прочитать. Этот бэкенд сможет обслуживать только id, перечисленные в поле «Модели».
backends-test-auth-failed = Отклонено с HTTP { $status }: учётные данные не приняты ({ $source }). Пока это не исправлено, обнаружение моделей отключено и бэкенд ничего не объявляет.
backends-test-http-error = { $url } ответил HTTP { $status }.
backends-test-upstream-said = Ответ upstream: { $detail }
backends-test-unreachable = Не удалось связаться с { $url }: { $err }
backends-test-timeout = { $url } не ответил за { $secs } с.
backends-test-key-typed = с введённым выше ключом
backends-test-key-stored = с сохранённым ключом
backends-test-key-env = из env { $var }
backends-test-key-env-unset = env { $var } НЕ ЗАДАНА — запрос ушёл без учётных данных
backends-test-key-none = учётные данные не отправлены
backends-error-base-url-required = требуется базовый URL

backends-parallel-mismatch = сервер обрабатывает { $parallel } одновременно
backends-detect-profile = определено: { $profile }

# OpenRouter catalog: one backend, one key, the account's models, and a
# purchase-price ceiling per selected model.
openrouter-heading = OpenRouter
openrouter-description = One OpenRouter connection. Pick the models this gateway may use and the maximum you will pay for each; a model with no provider under its ceiling is not offered.
openrouter-connected = Connected
openrouter-key-label = Key
openrouter-credits = Credits
openrouter-refresh = Refresh prices
openrouter-backend = Backend
openrouter-search = Search models
openrouter-select-all = Select all
openrouter-deselect-all = Deselect all
openrouter-selected-count = { $count } selected
openrouter-model = Model
openrouter-price = List price
openrouter-context = Context
openrouter-ceiling-prompt = Max input $/1M
openrouter-ceiling-completion = Max output $/1M
openrouter-strategy = Provider strategy
openrouter-strategy-cheapest = Cheapest
openrouter-strategy-fastest = Fastest
openrouter-strategy-balanced = Balanced
openrouter-min-throughput = Min tokens/s
openrouter-available = Available
openrouter-over-ceiling = Over ceiling
openrouter-save = Save selection
openrouter-saved = Selection saved.
openrouter-no-models = No models match this filter.
openrouter-error = OpenRouter: { $message }
openrouter-select-model-first = Select at least one model.
openrouter-endpoints = Providers
openrouter-provider = Provider
openrouter-add-heading = Добавить OpenRouter
openrouter-add-description = Один ключ — всё настроено: бэкенд плюс пул для каждого типа моделей (chat, transcription, speech, system_one, embedding, image, rerank), с именем openrouter-<тип>. Существующие пулы с таким именем добавляются. Выбор моделей и ценовые потолки — в панели OpenRouter.
openrouter-add-save = Добавить OpenRouter
openrouter-kinds = Типы моделей
openrouter-kind-selected-count = Выбрано: { $count }
openrouter-kind-catalog-count = { $count } в каталоге
openrouter-kind-not-enabled = Не включено
openrouter-key-change = Сменить ключ
openrouter-key-placeholder = Новый API-ключ
openrouter-key-save = Сохранить ключ
openrouter-key-saved = Ключ обновлён
openrouter-sort = Сортировка
openrouter-save-changed = Сохранить выбор · изменено: { $count }
openrouter-dirty-confirm = У «{ $kind }» есть несохранённые изменения. Отменить?
openrouter-provider-field = Provider
openrouter-provider-openai = OpenAI-compatible
openrouter-provider-openrouter = OpenRouter
openrouter-provider-hint = OpenRouter reads the account catalog and enforces the per-model price ceiling on every request. Other providers are plain OpenAI-compatible HTTP.
openrouter-ceiling-required = Every selected model needs both price ceilings.
openrouter-max-price = Max $/1M
openrouter-voices = Голоса
