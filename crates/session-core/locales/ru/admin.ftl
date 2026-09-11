# Strings owned by `gateway/src/rama_server/pages/admin.rs` — страница
# `/admin/models`.

admin-heading = Модели

admin-col-model = Модель

admin-not-configured = не настроено

admin-badge-ctx = КТХ

admin-save-model = Сохранить модель
admin-clear-overrides = Очистить все настройки
admin-cancel = Отмена

admin-toml-defaults-label = Значения сэмплирования (TOML)

# Цены по каждой модели для учёта расходов (цена за 1 млн токенов, ввод / вывод).
admin-price-in-label = Цена вх
admin-price-out-label = Цена вых
admin-price-in-placeholder = без цены
admin-price-out-placeholder = без цены

# Окно контекста (управляет авто-компактизацией).
admin-context-window-full-label = Окно контекста (токены)
admin-context-window-placeholder = по умолч.

# Модели по умолчанию для функций.
admin-defaults-heading = Модели по умолчанию

# Возможности модели (три состояния) + резервные модели.
admin-cap-tools = Инструменты

# Бэкенд веб-поиска (инструмент `search_web`).
admin-search-heading = Веб-поиск
admin-search-provider-label = Провайдер
admin-search-provider-searxng = SearXNG (самостоятельный хостинг)
admin-search-provider-brave = Brave Search API
admin-search-searxng-url-label = Базовый URL SearXNG
admin-search-searxng-url-placeholder = https://searxng.example.com
admin-search-brave-key-label = Ключ API Brave
admin-search-brave-key-placeholder = оставьте пустым, чтобы сохранить текущий ключ
admin-search-save = Сохранить веб-поиск
admin-search-saved = настройки веб-поиска сохранены

# ─── Админ-SPA на SvelteKit ──────────────────────────────────────────────────
# Проверка роли в оболочке админки, каталог рабочих процессов /admin/comfyui и
# те части редактора моделей, которых не было на старой странице.

admin-needs-admin-role = Для этих страниц нужна роль администратора.
admin-overwrite-existing = перезаписать существующий
admin-comfyui-reload = Перезагрузить каталог
admin-comfyui-reloaded = Каталог перезагружен — загружено процессов: { $count }.
admin-comfyui-empty = Рабочие процессы не загружены — проверьте каталог с содержимым.
admin-comfyui-heading = Каталог процессов ComfyUI
admin-comfyui-page-title = ComfyUI — Каталог рабочих процессов
admin-comfyui-intro = Фоновый worker ComfyUI. Шлюз предоставляет каждый загруженный процесс как инструмент comfyui_<id>, доступный модели. Пользователи не видят сам ComfyUI.
admin-comfyui-not-configured = Не настроено
admin-comfyui-not-configured-help = Включите ComfyUI в настройках, задайте базовый URL и каталог процессов, затем перезапустите шлюз.
admin-comfyui-operator-config = Конфигурация оператора
admin-comfyui-worker-url = Базовый URL worker
admin-comfyui-content-directory = Каталог содержимого
admin-comfyui-timeout = Тайм-аут процесса
admin-comfyui-poll-interval = Интервал опроса очереди
admin-comfyui-config-help = Каталог содержимого управляется оператором и не входит в публичный репозиторий. Измените манифесты и нажмите «Перезагрузить» — перезапуск не нужен.
admin-comfyui-loaded-workflows = Загруженные процессы
admin-comfyui-node = узел { $id }
admin-comfyui-workflow-meta = Название: { $title } · префикс: { $prefix }
admin-comfyui-parameters = Параметры
admin-comfyui-required = обязательно
admin-comfyui-recent-jobs = Недавние задания
admin-comfyui-pending = ожидают: { $count }
admin-comfyui-job-meta = prompt: { $prompt } · создано: { $created }
admin-comfyui-job-completed = завершено: { $completed }
admin-comfyui-reloaded-skipped = Каталог перезагружен — загружено процессов: { $count }, пропущено: { $skipped }.
admin-clear-overrides-confirm = Удалить все сохранённые переопределения для { $model }?
admin-cap-no-fallback = (нет)

admin-page-title = Модели — LLM Gateway

admin-intro-prefix = Настройки по каждой модели — цены, окно контекста, рассуждения, возможности и значения сэмплирования — применяются к

admin-intro-every = каждому

admin-intro-middle = запросу к этой модели от любого пользователя или токена, если только вызывающая сторона не задаст то же значение, которое

admin-intro-always-wins = всегда имеет приоритет

admin-intro-suffix = . Чат-модели, псевдонимы и другие виды — всё в одном списке.

admin-no-models = Пока нет доступных моделей. Как только появится доступный вышестоящий бэкенд, он отобразится здесь.

admin-filter-placeholder = Фильтр моделей…

admin-filter-all = Все

admin-filter-chat = чат

admin-filter-other = другие виды

admin-filter-aliases = псевдонимы

admin-filter-configured = только настроенные

admin-col-kind = Вид

admin-col-price = Цена вх/вых

admin-col-context = Контекст

admin-col-reasoning = Рассуждения

admin-col-configured = Настроено

admin-value-default = по умолчанию

admin-value-na = н/д

admin-alias-inherits = наследует настройки цели

admin-reasoning-auto-resolved = Авто → { $style }

admin-badge-price = ЦЕНА

admin-badge-budget = БЮДЖЕТ

admin-badge-caps = ВОЗМ

admin-badge-toml = TOML

admin-other-price-note = Сэмплирование, рассуждения и контекст к этому виду не применяются — только цены, для учёта расходов.

admin-toml-placeholder-header = # Общие ключи (vLLM/OpenAI):

admin-reasoning-style-label = Стиль рассуждений

admin-reasoning-style-aria = Стиль рассуждений

admin-reasoning-auto = Авто

admin-reasoning-none = нет

admin-reasoning-qwen = Qwen (vLLM)

admin-reasoning-openai = OpenAI

admin-reasoning-glm = GLM / z.AI

admin-reasoning-anthropic = Anthropic

admin-effort-standard = Стандартный

admin-effort-deep = Глубокий

admin-effort-max = Макс

admin-budget-placeholder = по умолчанию

admin-budget-hint = Максимум токенов на размышления для каждого уровня. Пусто = значение бэкенда по умолчанию (без ограничений). «Fast» отключает рассуждения.

admin-effort-default-option = (по умолчанию)

admin-effort-hint = Уровень усилий для рассуждений по каждому уровню. Пусто = встроенное значение по умолчанию. «Fast» отключает рассуждения.

admin-saved-model = `{ $model }` сохранено — вступает в силу немедленно

admin-cleared-defaults = настройки для `{ $model }` очищены

admin-price-label = { $cur }/{ $unit }

admin-price-unit-tokens = 1 млн токенов

admin-price-unit-images = изображение

admin-price-unit-characters = символ

admin-price-unit-seconds = секунда

admin-alias-chip = псевдоним

admin-defaults-intro = Выберите модель, предварительно выбранную для каждой функции. Пусто = первая доступная модель (прежнее поведение).

admin-defaults-chat-label = Чат

admin-defaults-voice-label = Голос (транскрипция)

admin-defaults-image-label = Генерация изображений

admin-defaults-embedding-label = Эмбеддинги (RAG)

admin-defaults-first-option = Первая доступная

admin-defaults-saved = модель по умолчанию установлена: `{ $model }`

admin-defaults-cleared = модель по умолчанию сброшена

admin-capabilities-heading = Возможности

admin-cap-vision = Зрение

admin-cap-structured-output = Структурированный вывод

admin-cap-audio-input = Аудиовход

admin-cap-pdf-input = Ввод PDF

admin-cap-parallel-tools = Параллельные инструменты

admin-cap-unknown = Неизвестно

admin-cap-enabled = Включено

admin-cap-disabled = Отключено

admin-cap-fallback-vision = Резерв для зрения

admin-cap-fallback-tools = Резерв для инструментов

admin-search-intro = Какой бэкенд отвечает на инструмент `search_web`. SearXNG требует только базовый URL и не стоит ничего за запрос, если вы поднимаете свой экземпляр; Brave требует ключ API. Ключ шифруется при хранении.

admin-search-brave-key-set = Ключ сохранён (в зашифрованном виде).

admin-search-brave-key-unset = Ключ не сохранён.

admin-search-brave-key-clear = Удалить сохранённый ключ
