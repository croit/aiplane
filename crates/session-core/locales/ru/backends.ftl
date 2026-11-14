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
backends-field-api-key-clear = Удалить сохранённый ключ

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
backends-aliases-managed = Алиасы редактируются на вкладке «Алиасы».
backends-field-extra-headers = Дополнительные заголовки (имя: значение на строку)
backends-field-extra-headers-hint = Отправляется с каждым запросом к этому бэкенду, после API-ключа. Например anthropic-version: 2023-06-01, который требует Anthropic.
backends-field-supports-edit = Поддерживает редактирование изображений
backends-no-models-title = Этот бэкенд не объявляет ни одной модели, поэтому на него ничего не маршрутизируется, а голому псевдониму не к чему привязаться. Обычно это проба, которая ни разу не вернула данные.
backends-no-models = нет моделей
backends-key-env-badge = ключ: env { $var }
backends-key-env-unset-badge = env { $var } НЕ ЗАДАНА
backends-enabled-hint = Выключите, чтобы вывести бэкенд на обслуживание. Действует сразу. Его модели остаются известными, поэтому нагрузку берут другие бэкенды пула, а клиенты видят временную недоступность, а не «модель не найдена».
backends-enabled-label = Принимает трафик ({ $name })
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
backends-test-models = Сообщённые id моделей:
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
openrouter-description = Одно подключение OpenRouter. Выберите модели, которые может использовать этот шлюз, и при необходимости предел цены для каждой; модель без подходящего провайдера не предлагается.
openrouter-connected = Подключено
openrouter-key-label = Ключ
openrouter-credits = Баланс
openrouter-refresh = Обновить цены
openrouter-backend = Бэкенд
openrouter-search = Поиск моделей
openrouter-select-all = Выбрать все
openrouter-deselect-all = Снять все
openrouter-selected-count = Выбрано: { $count }
openrouter-model = Модель
openrouter-models = Модели
openrouter-filter-all = Все
openrouter-filter-selected = Выбранные
openrouter-filter-fitting = Подходящие
openrouter-price = Цена по каталогу
openrouter-context = Контекст
openrouter-ceiling-prompt = Макс. вход $/1M
openrouter-in = Вход
openrouter-out = Выход
openrouter-max-input = Макс. вход
openrouter-max-output = Макс. выход
openrouter-provider-input-second = Вход $/с
openrouter-provider-output-second = Выход $/с
openrouter-ceiling-prompt-second = Макс. вход $/с
openrouter-ceiling-completion-second = Макс. выход $/с
openrouter-max-price-second = Макс. $/с
openrouter-cheapest-second = Дешевле всех: { $provider } — { $prompt } / { $completion } за секунду аудио
openrouter-per-million = /1M
openrouter-per-second = /с
openrouter-show-models = Показать модели
openrouter-hide-models = Скрыть модели
openrouter-ceiling-invalid = Ценовые пределы должны быть пустыми или неотрицательным числом.
openrouter-ceiling-completion = Макс. выход $/1M
openrouter-strategy = Стратегия провайдера
openrouter-strategy-cheapest = Самая дешёвая
openrouter-strategy-fastest = Самая быстрая
openrouter-strategy-balanced = Сбалансированная
openrouter-min-throughput = Мин. токенов/с
openrouter-over-ceiling = Дороже лимита
openrouter-save = Сохранить выбор
openrouter-saved = Выбор сохранён.
openrouter-no-models = Ни одна модель не подходит под этот фильтр.
openrouter-error = OpenRouter: { $message }
openrouter-select-model-first = Выберите хотя бы одну модель.
openrouter-endpoints = Провайдеры
openrouter-provider-input = Вход $/1M
openrouter-provider-output = Выход $/1M
openrouter-provider-cache-read = Чтение кэша
openrouter-provider-throughput = tok/s
openrouter-provider-latency = Задержка
openrouter-provider-uptime = Доступность
openrouter-provider-limit = Твой лимит
openrouter-usage = Расход
openrouter-aria-input-ceiling = { $model } лимит на вход
openrouter-aria-output-ceiling = { $model } лимит на выход
openrouter-aria-strategy = { $model } стратегия провайдера
openrouter-aria-min-throughput = { $model } минимальная пропускная способность
openrouter-cheapest-provider = Дешевле всех
openrouter-provider-fits = подходит
openrouter-provider-excluded = дороже
openrouter-providers-fit = Доступно { $fit } из { $total } провайдеров
openrouter-providers-hidden = { $count ->
    [one] { $count } поставщик скрыт из-за лимитов
    [few] { $count } поставщика скрыто из-за лимитов
    [many] { $count } поставщиков скрыто из-за лимитов
   *[other] { $count } поставщиков скрыто из-за лимитов
  }
openrouter-providers-none-fit = Ни один провайдер не подходит под ваши пределы — увеличьте или очистите «Макс. вход» / «Макс. выход».
openrouter-providers-fit-short = { $fit } из { $total } подходят
openrouter-price-unknown = цена не проверена
openrouter-price-unknown-hint = Цены провайдеров для этой модели ещё не загружены — открой строку, чтобы увидеть их.
openrouter-cheapest = Дешевле всех: { $provider } — { $prompt } / { $completion } за 1M
openrouter-use-cheapest = Взять самый дешёвый
openrouter-compare = Сравнить на OpenRouter
openrouter-provider-error = Не удалось загрузить провайдеров: { $message }
openrouter-provider-none = OpenRouter не сообщает провайдеров для этой модели.
openrouter-provider = Провайдер
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
openrouter-provider-field = Провайдер
openrouter-provider-openai = Совместимый с OpenAI
openrouter-provider-openrouter = OpenRouter
openrouter-provider-hint = OpenRouter читает каталог аккаунта и применяет лимит цены для каждой модели при каждом запросе. Остальные провайдеры — обычный HTTP, совместимый с OpenAI.
openrouter-max-price = Макс. $/1M
openrouter-voices = Голоса
