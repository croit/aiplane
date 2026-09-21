# Strings owned by `gateway/src/rama_server/pages/admin.rs` — страница
# `/admin/models`.

admin-heading = Модели
admin-models-routing-heading = Модели и маршрутизация
admin-models-routing-intro = Настройте инфраструктуру моделей, каталог, модели по умолчанию и автоматическую маршрутизацию в одном месте.
admin-models-tab-upstreams = Апстримы
admin-models-tab-catalog = Каталог моделей
admin-models-tab-defaults = Модели по умолчанию
admin-models-tab-routing = Автоматическая маршрутизация

admin-col-model = Модель

admin-not-configured = не настроено

admin-badge-ctx = КТХ

admin-save-model = Сохранить модель
admin-edit-model = Изменить
admin-edit-model-page-title = Изменить модель
admin-model-not-found = Такой модели нет. Возможно, её больше не предоставляет ни один бэкенд.
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
admin-comfyui-parameters = Параметры
admin-comfyui-required = обязательно
admin-comfyui-recent-jobs = Недавние задания
admin-comfyui-reloaded-skipped = Каталог перезагружен — загружено процессов: { $count }, пропущено: { $skipped }.
admin-comfyui-max-concurrent = Одновременных задач
admin-comfyui-tab-workflows = Рабочие процессы
admin-comfyui-tab-jobs = Запуски
admin-comfyui-jobs-page-title = ComfyUI — Запуски
admin-comfyui-jobs-intro = Все недавно выполненные моделями рабочие процессы, новые сверху: что получилось, сколько заняло и из какого разговора пришёл запрос.
admin-comfyui-jobs-window = Последние { $count } запусков, записанных шлюзом.
admin-comfyui-jobs-empty = Запусков пока нет.
admin-comfyui-worker-status = Воркер
admin-comfyui-worker-reachable = Доступен
admin-comfyui-worker-unreachable = Недоступен
admin-comfyui-worker-checking = Проверка…
admin-comfyui-worker-queue = { $running } выполняется · { $pending } в очереди
admin-comfyui-worker-software = ComfyUI { $version } · Python { $python } · PyTorch { $torch }
admin-comfyui-worker-vram = { $free } свободно из { $total }
admin-comfyui-search-placeholder = Поиск по рабочим процессам и параметрам
admin-comfyui-search-empty = Ни один рабочий процесс не подходит под этот запрос.
admin-comfyui-filename-prefix = Префикс вывода
admin-comfyui-required-count = { $required } из { $total } обязательны
admin-comfyui-no-params = У этого рабочего процесса нет параметров.
admin-comfyui-detail-empty = Выберите рабочий процесс, чтобы увидеть контракт, который получает модель.
admin-comfyui-param-column = Параметр
admin-comfyui-param-description-column = Описание
admin-comfyui-filter-all = Все
admin-comfyui-filter-completed = Завершённые
admin-comfyui-filter-pending = В работе
admin-comfyui-filter-failed = Неудачные
admin-comfyui-stats-heading = Надёжность по рабочим процессам
admin-comfyui-col-workflow = Рабочий процесс
admin-comfyui-col-runs = Запуски
admin-comfyui-col-failed = Ошибки
admin-comfyui-col-median = Медиана
admin-comfyui-col-status = Статус
admin-comfyui-col-duration = Длительность
admin-comfyui-col-when = Начало
admin-comfyui-col-result = Результат
admin-comfyui-job-open = Открыть разговор
admin-comfyui-job-still-running = ещё выполняется
admin-comfyui-refresh = Обновить
admin-clear-overrides-confirm = Удалить все сохранённые переопределения для { $model }?
admin-cap-no-fallback = (нет)

admin-page-title = Модели — AIplane

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
admin-reasoning-ollama = Ollama

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

# Источник значения окна контекста.
admin-context-detected = Определено: { $window } токенов
admin-context-unreported = Этот бэкенд не сообщает окно контекста для этой модели. Укажите его здесь или проверьте на сервере.
admin-context-exceeds-detected = Этот бэкенд сообщает { $window } токенов. Большие значения не уплотняются — сервер молча обрезает их.
auto-route-heading = Автоматическая маршрутизация моделей
auto-route-description = Создаёт стандартный псевдоним модели, выбирающий лучшую разрешённую модель для каждого запроса.
auto-route-add = Добавить автоматический маршрут
auto-route-privacy = Селектор получает содержимое запроса для классификации, даже если выбранная модель генерации работает локально.
auto-route-empty = Автоматические маршруты не настроены.
auto-route-needs-selector = Добавьте upstream System One перед созданием автоматического маршрута.
auto-route-needs-candidates = Добавьте как минимум две разные чат-модели перед созданием автоматического маршрута.
auto-route-open-upstreams = Открыть upstream
auto-route-candidates = Кандидаты
auto-route-edit = Изменить
auto-route-delete = Удалить
auto-route-delete-confirm = Удалить автоматический маршрут «{ $alias }»?
auto-route-deleted = Автоматический маршрут «{ $alias }» удалён.
auto-route-saved = Автоматический маршрут «{ $alias }» сохранён.
auto-route-alias = Псевдоним модели
auto-route-alias-help = Клиенты передают это значение в стандартном поле модели.
auto-route-selector = Модель-селектор
auto-route-selector-help = Модель из upstream System One, которая выбирает ключ кандидата.
auto-route-editor-help = Начните с теневого режима, проверьте решения маршрутизации и активируйте маршрут, когда его политика будет готова.
auto-route-objective = Цель оптимизации
auto-route-objective-quality = Качество
auto-route-objective-balanced = Баланс
auto-route-objective-cost = Стоимость
auto-route-rollout = Режим запуска
auto-route-rollout-shadow = Теневой (только измерение)
auto-route-rollout-active = Активный
auto-route-rollout-help = Теневой режим записывает решения, но направляет трафик на резервную модель. Активный режим применяет решение селектора.
auto-route-confidence = Минимальная уверенность
auto-route-confidence-help = Решения с меньшей уверенностью используют резервную цель.
auto-route-timeout = Тайм-аут селектора (мс)
auto-route-instructions = Инструкции маршрутизации
auto-route-instructions-help = Необязательные правила для селектора на обычном языке. Специальный синтаксис не нужен.
auto-route-instructions-placeholder = Пример: выбирайте expert для сложного анализа и кода; используйте fast для коротких типовых запросов.
auto-route-candidate-add = Добавить кандидата
auto-route-candidates-help = Опишите каждую модель через запросы, которые она должна обрабатывать. Селектор видит ключ и описание, но не имя целевой модели.
auto-route-candidate-key = Непрозрачный ключ
auto-route-candidate-target = Целевая модель или статический псевдоним
auto-route-candidate-description = Когда следует использовать кандидата
auto-route-candidate-description-help = Обязательный свободный текст. Опишите запросы, с которыми этот кандидат справляется лучше всего; селектор видит этот текст, а не имя целевой модели.
auto-route-candidate-description-placeholder = Пример: короткие типовые запросы, где особенно важны низкая задержка и стоимость.
auto-route-fallback = Резервная цель
auto-route-session-affinity = Закрепить сессию за первой выбранной моделью
auto-route-session-affinity-help = Повторно использует фактическую цель для того же авторизованного клиента и сеанса до истечения TTL.
auto-route-session-ttl = TTL закрепления сессии (секунды)
auto-route-cancel = Отмена
auto-route-save = Сохранить маршрут
auto-route-saving = Сохранение…
auto-route-decisions = Последние решения маршрутизации
auto-route-result = Фактическая цель
auto-route-latency = Задержка селектора
auto-route-reason-selected = Выбрано
auto-route-reason-shadow = Теневое предложение
auto-route-reason-low-confidence = Низкая уверенность
auto-route-reason-selector-error = Ошибка селектора
auto-route-reason-session-affinity = Привязка к сеансу
auto-route-error-unavailable = Ни одна настроенная цель сейчас не может обработать этот запрос. Проверьте доступность и возможности кандидатов и повторите попытку.
auto-route-error-internal = Не удалось загрузить автоматический маршрут. Проверьте журналы шлюза и повторите попытку.
auto-route-error-invalid-body = Запрос автоматического маршрута сформирован неверно. Проверьте отправленные поля и повторите попытку.
auto-route-error-invalid-config = Конфигурация автоматического маршрута недопустима. Проверьте кандидатов, резервную модель, уверенность и тайм-ауты.
auto-route-error-nested = Автоматические маршруты не могут указывать на другие автоматические маршруты. Выберите статические псевдонимы или идентификаторы моделей.
auto-route-error-missing-alias = В URL запроса отсутствует псевдоним автоматического маршрута.
auto-route-error-not-found = Автоматический маршрут не существует. Обновите список маршрутов и повторите попытку.
admin-error-unknown-groups = { $count ->
    [one] Здесь нет такой группы: { $groups }. Сначала создайте её в разделе «Администрирование → Группы» или выберите из списка.
   *[other] Здесь нет таких групп: { $groups }. Сначала создайте их в разделе «Администрирование → Группы» или выберите из списка.
}
