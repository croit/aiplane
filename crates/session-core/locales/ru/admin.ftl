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
admin-comfyui-reloaded = Перезагружено рабочих процессов: { $count }.
admin-comfyui-empty = Рабочие процессы не загружены — проверьте каталог с содержимым.
admin-defaults-model-aria = Модель по умолчанию для { $feature }
admin-defaults-set = Задать
admin-search-provider-none = Нет
admin-add-overrides-heading = Добавить переопределения модели
admin-edit-model-heading = Изменить { $model }
admin-add-model = Добавить…
admin-pricing-unit-label = Единица тарификации
admin-pricing-unit-mtok = за Mtok
admin-pricing-unit-ktok = за Ktok
admin-pricing-unit-kimgs = за 1000 изображений
admin-clear-overrides-confirm = Удалить все сохранённые переопределения для { $model }?
admin-users-col-email = Эл. почта
