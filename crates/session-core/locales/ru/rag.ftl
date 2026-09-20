# STATUS: llm-generated, unreviewed — pending native-speaker QA

rag-heading = Коллекции RAG
rag-description-prefix = Кодовые базы, проиндексированные шлюзом. Инструмент
rag-description-suffix = обращается к этим коллекциям, чтобы отвечать на вопросы о коде.
rag-collections-heading = Настроенные коллекции
rag-empty-list = Пока нет коллекций. Создайте одну выше.

# Toasts — collection CRUD
rag-toast-indexing-queued = Индексация `{ $name }` @ `{ $ref }` поставлена в очередь.
rag-toast-created-aggregate = `{ $name }` создана (агрегат). Добавьте исходные репозитории ниже, чтобы их проиндексировать.
rag-toast-collection-saved = `{ $name }` сохранена.
rag-toast-vanished = Коллекция исчезла после сохранения.
# Toasts — refs / sources
rag-toast-bulk-queued-skipped = Поставлено в очередь { $added } источник(ов); пропущено { $skipped } дубликат(ов).
rag-toast-bulk-queued = Индексация { $added } источник(ов) поставлена в очередь.
rag-toast-source-updated = Источник обновлён.

# Status badges
rag-status-pending = в ожидании
rag-status-unconfigured = нет источников
rag-status-cloning = клонирование
rag-status-indexing = индексация
rag-status-ready = готово
rag-status-error = ошибка

# Collection row
rag-pat-set = PAT задан
rag-pat-none = нет PAT
rag-meta-aggregate = { $count } источник(ов) · { $hint }
rag-meta-versioned = { $url } · { $hint }
rag-badge-aggregate = агрегат
rag-embed-prefix = embed:
rag-button-edit = Изменить
rag-button-delete-collection = Удалить коллекцию
rag-placeholder-source-git-url = https://github.com/org/repo.git
rag-button-add-source = Добавить источник
rag-placeholder-branch-tag-commit = ветка, тег или коммит
rag-button-add-ref = Добавить ref
rag-placeholder-bulk-sources = Массовое добавление — один репозиторий на строку, необязательный @ref:
    https://github.com/proxmox/pve-manager.git
    https://github.com/proxmox/qemu-server.git @master
rag-button-add-bulk = Добавить источники (массово)

# Ref / source row
rag-badge-primary = основной
rag-ref-indexed-line = проиндексировано { $date } · { $commit }
rag-never = никогда
rag-button-log = Журнал
rag-button-reindex = Переиндексировать
rag-button-set-primary = Сделать основным
rag-button-remove = Удалить

# Indexing log
rag-log-info = инфо
rag-log-warn = предупр.
rag-log-error = ошибка
rag-log-heading = Журнал индексации
rag-log-empty = Событий индексации пока не зафиксировано. Первый запуск появится здесь, как только индексатор возьмётся за этот ref.

# Inline per-source editor
rag-label-git-url-source = URL Git (этот источник)
rag-label-git-url-inherit = URL Git (пусто = унаследовать от коллекции)
rag-placeholder-git-url = https://example.com/org/repo.git
rag-label-branch-tag = Ветка / тег
rag-button-save-source = Сохранить источник
rag-button-cancel = Отмена

# Create-collection form
rag-create-heading = Индексировать новую коллекцию
rag-create-description = Индексатор клонирует репозиторий, разбивает каждый файл на чанки и создаёт для них embedding с помощью настроенной модели. PAT хранятся в открытом виде (шлюз работает на доверенной инфраструктуре).
rag-new-page-title = Индексировать новую коллекцию
rag-edit-page-title = Изменить коллекцию
rag-not-found = Коллекция с таким id не найдена. Возможно, она была удалена.
rag-back-to-collections = Коллекции RAG
rag-edit-source-heading = Изменить источник
rag-add-source-heading = Добавить источник
rag-label-name = Имя
rag-placeholder-name = напр. gateway-repo
rag-label-description-optional = Описание (необязательно)
rag-placeholder-description = кратко, для людей
rag-label-git-url-versioned = URL Git (только для версионированных)
rag-label-pat-optional = Персональный токен доступа (необязательно)
rag-placeholder-pat = для приватных репозиториев
rag-label-include-globs-full = Include-шаблоны (через запятую или с новой строки)
rag-placeholder-include-globs = *.rs, *.md
rag-label-exclude-globs = Exclude-шаблоны
rag-placeholder-exclude-globs = target/, node_modules/
rag-label-chunk-size = Размер чанка
rag-label-chunk-overlap = Перекрытие чанков
rag-label-refresh-interval = Автоматическая пересинхронизация
rag-hint-refresh-interval = Как часто переиндексировать без запроса. Источник, который не может уведомить шлюз, — архив рассылки, обычная WebDAV-шара — свеж ровно настолько.
rag-refresh-never = Никогда (вручную или через sync-хук)
rag-refresh-hourly = Каждый час
rag-refresh-daily = Каждый день
rag-refresh-weekly = Каждую неделю
rag-refresh-custom = Каждые { $mins } мин.
rag-create-aggregate-help = Агрегат (несколько источников): поиск по многим репозиториям как по единому корпусу. Оставьте URL Git пустым и добавьте каждый исходный репозиторий после создания. Ветка/тег станет ref по умолчанию для добавленных источников.
rag-button-queue-indexing = Поставить индексацию в очередь

# Edit-collection form
rag-edit-heading = Редактирование { $name }
rag-label-description = Описание
rag-label-pat = Персональный токен доступа
rag-placeholder-pat-keep = оставьте пустым, чтобы сохранить текущий
rag-label-clear-pat = Удалить сохранённый PAT (прекратить аутентификацию)
rag-label-include-globs = Include-шаблоны
rag-button-save-changes = Сохранить изменения

# Embedding model field
rag-label-embedding-model = Модель embedding
rag-placeholder-embedding-model-none = пулы embedding не настроены — введите id модели
rag-option-choose-embedding-model = Выберите модель embedding…
rag-suffix-not-advertised = (больше не предлагается)

rag-label-allowed-groups = Разрешённые группы
rag-hint-allowed-groups = Группы AIplane (через запятую), которым разрешено просматривать и искать в этой коллекции. Пусто = все, у кого есть RAG-инструменты. Админы всегда имеют доступ.

# Выбор источника + учётные данные провайдера (rag_source.rs). Подписи
# полей задаёт сам провайдер, они не переводятся.
rag-label-source-kind = Источник
rag-source-git-help = Клонирует репозиторий и индексирует его файлы. Исходное поведение.
rag-source-secret-placeholder = оставьте пустым, чтобы сохранить текущее значение
rag-source-unknown-kind = Неизвестный тип источника.
rag-source-test-button = Проверить подключение
rag-source-test-ok = Подключено как `{ $account }`. Элементов в указанной папке: { $entries }.
rag-source-test-ok-plain = Подключено. Элементов в указанной папке: { $entries }.
rag-source-test-failed = Не удалось получить доступ к источнику: { $error }
rag-source-test-git = Выберите удалённый источник для проверки. Git-репозитории проверяются при индексации.
rag-source-detected = Обнаружено: { $server }

rag-label-profile = Поля документа
rag-option-profile-none = Нет — индексировать только текст
rag-profile-help = Извлекает поля (поставщик, дата, сумма, проект) из каждого документа, чтобы их можно было фильтровать, сортировать и суммировать. Стоит одного вызова модели на документ; для кода и обычного текста оставьте «Нет».

# Редактор профилей извлечения (/rag/profiles, rag_profiles.rs)
rag-profile-heading = Профили извлечения
rag-profile-description = Что извлекается из каждого документа коллекции: поля, благодаря которым вопросы «последний счёт от X» или «сколько мы потратили» вообще получают ответ. Профиль назначается коллекции на странице RAG.
rag-profile-create-heading = Новый профиль
rag-profile-list-heading = Профили
rag-profile-empty = Профилей пока нет.
rag-profile-builtin = встроенный
rag-profile-version = v{ $version }
rag-profile-summary = полей: { $count }
rag-profile-label-name = Название
rag-profile-label-description = Описание
rag-profile-label-prompt = Инструкции по извлечению
rag-profile-label-fields = Поля (JSON)
rag-profile-prompt-placeholder = Опишите, что читает модель и как нормализовать даты и суммы.
rag-profile-fields-help = По одному объекту на поле: key, label, type (text | number | date | enum), description и при необходимости filterable / sortable. Для enum нужен также список «values». Описание видит модель — формулируйте точно.
rag-profile-edit-warning = Сохранение повышает версию профиля и очищает кэш извлечений. Коллекции, использующие профиль, нужно переиндексировать, чтобы применить новые поля.
rag-profile-button-create = Создать профиль
rag-profile-button-save = Сохранить
rag-profile-button-delete = Удалить
rag-profile-delete-confirm = Удалить профиль { $name }?
rag-profile-example-counterparty-label = Контрагент
rag-profile-example-counterparty-description = Другая сторона.
rag-profile-example-date-label = Дата
rag-profile-example-date-description = Дата документа.
rag-profile-example-amount-label = Сумма
rag-profile-example-amount-description = Общая сумма.
rag-profile-link = Редактировать профили извлечения
rag-profile-toast-created = Профиль «{ $name }» создан.
rag-profile-toast-saved = «{ $name }» сохранён.
rag-profile-toast-saved-reindex = «{ $name }» сохранён. Переиндексируйте, чтобы применить: { $collections }.
rag-profile-toast-deleted = Профиль удалён.
# Sync-хук — входящий триггер, запускающий пересинхронизацию коллекции.
rag-toast-sync-token = URL синхронизации (показывается один раз и не сохраняется): { $url }
rag-toast-sync-token-cleared = URL синхронизации отключён.
rag-button-sync-token = URL синхронизации
rag-button-sync-token-rotate = Новый URL синхронизации
rag-button-sync-token-clear = Отключить URL синхронизации
rag-badge-sync-hook = sync-хук

# Browser consent for an OAuth source (Google Drive).
rag-source-consent-save-first = Сначала сохраните коллекцию с ID клиента и секретом, затем подключите её, чтобы предоставить доступ.
rag-source-consent-connected = подключено
rag-source-consent-connect = Подключить
rag-oauth-lookup-failed = Не удалось прочитать коллекцию.
rag-oauth-not-oauth = Этот тип источника не подключается через браузер.
rag-oauth-no-client = Сначала сохраните OAuth ID клиента и секрет в коллекции.
rag-oauth-bad-authorize-url = Не удалось построить URL авторизации провайдера.
rag-oauth-start-failed = Не удалось начать авторизацию.
rag-oauth-callback-missing = В ответе провайдера отсутствовал код или state.
rag-oauth-expired = Эта авторизация истекла или уже использована. Начните заново.
rag-oauth-provider-refused = Провайдер отклонил авторизацию: { $error }
rag-oauth-exchange-failed = Не удалось обменять код авторизации: { $error }
rag-oauth-no-refresh-token = Провайдер не вернул refresh-токен, поэтому автономная индексация невозможна. Отзовите доступ шлюза в учётной записи провайдера и подключитесь снова.
rag-oauth-store-failed = Не удалось сохранить учётные данные.
rag-badge-no-files = файлы не проиндексированы
rag-ref-files = файлов: { $files }
rag-label-git-url = URL Git
rag-source-testing = Проверка…
rag-sync-url-heading = URL синхронизации — показывается один раз
rag-sync-token-confirm = Создать новый URL синхронизации? Старый перестанет работать.
rag-delete-collection-confirm = Удалить коллекцию { $name } вместе с её индексом?
rag-remove-source-confirm = Удалить источник { $source }?
rag-toast-rebuild-queued = Запрошена полная пересборка индекса.
rag-no-sources = Источников нет — коллекция ничего не индексирует, пока не добавлен хотя бы один.
rag-add-sources-hint = По одному источнику в строке; добавьте { $at }, чтобы переопределить ref { $ref } этой коллекции.
