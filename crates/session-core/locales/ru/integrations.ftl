# STATUS: llm-generated, unreviewed — pending native-speaker QA

integrations-heading = Интеграции
integrations-intro = Подключите свои собственные учётные записи, чтобы ассистент мог действовать от вашего имени — читать вашу почту, календарь, файлы, репозитории и многое другое. Каждое подключение использует ваши собственные права доступа и может быть отключено в любой момент.
integrations-empty = Пока нет доступных коннекторов. Администратор может включить их в разделе Админ → Коннекторы.

integrations-badge-connected = Подключено

integrations-disconnect-button = Отключить
integrations-disconnect-confirm = Отключить эту интеграцию? Сохранённый токен доступа будет удалён.
integrations-connect-button = Подключить

integrations-token-label = Ваш API-токен
integrations-token-placeholder = вставьте ваш токен

integrations-error-unknown-connector = неизвестный или отключённый коннектор
integrations-error-forbidden-role = у вас нет доступа к этому коннектору
integrations-error-not-oauth = этот коннектор не использует OAuth
integrations-error-oauth-discovery-failed = не удалось обнаружить параметры OAuth: { $error }
integrations-error-needs-setup-no-client = этот коннектор требует настройки: не настроен id клиента, а провайдер не предлагает динамическую регистрацию. Попросите администратора добавить клиент OAuth.
integrations-error-sealing-client-secret = запечатывание секрета клиента: { $error }
integrations-error-dcr-failed = динамическая регистрация клиента не удалась: { $error }
integrations-error-needs-setup-admin = этот коннектор требует настройки: администратор должен настроить id клиента OAuth.
integrations-error-building-authorize-url = построение URL авторизации: { $error }
integrations-error-persisting-authorization = сохранение авторизации: { $error }
integrations-error-provider-error = провайдер вернул ошибку: { $error } { $desc }
integrations-error-callback-missing = в обратном вызове отсутствует код или состояние
integrations-error-auth-expired = срок действия этой авторизации истёк или она уже использована — начните заново со страницы «Интеграции»
integrations-error-loading-authorization = загрузка авторизации: { $error }
integrations-error-state-mismatch = состояние авторизации не совпало с вашей сессией
integrations-error-connector-missing = коннектор больше не существует
integrations-error-decrypting-client-secret = расшифровка секрета клиента: { $error }
integrations-error-connector-missing-client-id = у коннектора отсутствует id клиента OAuth
integrations-error-sealing-access-token = запечатывание токена доступа: { $error }
integrations-error-sealing-refresh-token = запечатывание токена обновления: { $error }
integrations-error-saving-connection = сохранение подключения: { $error }

# SPA-only: the Svelte /integrations connector list.
integrations-badge-not-connected = Не подключено
integrations-toast-connected = { $name } подключён.
