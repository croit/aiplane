# STATUS: llm-generated, unreviewed — pending native-speaker QA

rag-heading = Коллекции RAG

# Toasts — collection CRUD
rag-toast-vanished = Коллекция исчезла после сохранения.

# Toasts — refs / sources
rag-toast-bulk-queued-skipped = Поставлено в очередь { $added } источник(ов); пропущено { $skipped } дубликат(ов).
rag-toast-bulk-queued = Индексация { $added } источник(ов) поставлена в очередь.
rag-toast-reindex-queued-ref = Переиндексация `{ $ref }` поставлена в очередь.

# Status badges
rag-status-pending = в ожидании
rag-status-cloning = клонирование
rag-status-indexing = индексация
rag-status-ready = готово
rag-status-error = ошибка

# Collection row
rag-button-edit = Изменить
rag-button-add-source = Добавить источник
rag-button-add-bulk = Добавить источники (массово)

# Ref / source row
rag-badge-primary = основной
rag-button-reindex = Переиндексировать
rag-button-set-primary = Сделать основным
rag-button-remove = Удалить

# Inline per-source editor
rag-label-branch-tag = Ветка / тег
rag-button-cancel = Отмена

# Create-collection form
rag-label-name = Имя
rag-label-chunk-size = Размер чанка
rag-label-chunk-overlap = Перекрытие чанков

# Edit-collection form
rag-label-description = Описание

# Embedding model field
rag-label-embedding-model = Модель embedding

# Выбор источника + учётные данные провайдера (rag_source.rs). Подписи
# полей задаёт сам провайдер, они не переводятся.
rag-label-source-kind = Источник
rag-source-unknown-kind = Неизвестный тип источника.
rag-source-test-button = Проверить подключение
rag-source-test-ok = Подключено как `{ $account }`. Элементов в указанной папке: { $entries }.
rag-source-test-ok-plain = Подключено. Элементов в указанной папке: { $entries }.
rag-source-test-failed = Не удалось получить доступ к источнику: { $error }
rag-source-detected = Обнаружено: { $server }

rag-label-profile = Поля документа
rag-option-profile-none = Нет — индексировать только текст

# Sync-хук — входящий триггер, запускающий пересинхронизацию коллекции.
rag-button-sync-token = URL синхронизации
rag-badge-sync-hook = sync-хук

# Browser consent for an OAuth source (Google Drive).
rag-source-consent-save-first = Сначала сохраните коллекцию с ID клиента и секретом, затем подключите её, чтобы предоставить доступ.
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

# Управление коллекциями в SPA: форма создания, карточка URL синхронизации,
# строки источников и подтверждения, которых не было на старой странице.
rag-button-new-collection = Новая коллекция
rag-label-git-url = URL Git
rag-source-testing = Проверка…
rag-button-create = Создать
rag-button-rebuild = Пересобрать
rag-sync-url-heading = URL синхронизации — показывается один раз
rag-sync-token-confirm = Создать новый URL синхронизации? Старый перестанет работать.
rag-delete-collection-confirm = Удалить коллекцию { $name } вместе с её индексом?
rag-remove-source-confirm = Удалить источник { $source }?
rag-toast-rebuild-queued = Запрошена полная пересборка индекса.
rag-ref-indexed-at = проиндексировано { $date }
rag-no-sources = Источников нет — коллекция ничего не индексирует, пока не добавлен хотя бы один.
rag-add-sources-hint = По одному источнику в строке; добавьте { $at }, чтобы переопределить ref { $ref } этой коллекции.
