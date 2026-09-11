# STATUS: llm-generated, unreviewed — pending native-speaker QA


backends-status-down = недоступен
backends-status-up = активен

backends-inflight-label = в обработке { $load }

# Backend CRUD editor (add/edit/delete backends stored in the DB topology).
backends-apply-changes = Применить изменения
backends-field-name = Название
backends-field-base-url = Базовый URL
backends-field-pool = Пул
backends-field-pool-none = (нет)
backends-save-backend = Сохранить бэкенд
backends-delete-backend = Удалить

backends-field-api-key = Ключ API
backends-field-api-key-keep = оставьте пустым, чтобы сохранить текущий ключ

# An alias that is configured but would not route.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.
backends-status-drained = обслуживание

# Строки бэкендов в SPA: переключатель обслуживания, нагрузка за час и удаление.
backends-add-heading = Добавить бэкенд
backends-name-taken = Бэкенд с таким именем уже существует — сохранение перезапишет его вместе с базовым URL, ключом, моделями и пулом. Выберите другое имя, чтобы добавить второй бэкенд.
backends-field-api-key-placeholder = Ключ API (хранится в зашифрованном виде)
backends-field-api-key-env = Переменная окружения с API-ключом
backends-field-health-path = Путь проверки состояния
backends-field-pool-hint = Назначает этот бэкенд одному пулу. Бэкенд в нескольких пулах сводится к выбранному здесь.
backends-field-weight = Вес
backends-field-max-inflight = Макс. одновременных
backends-field-models = Модели (через запятую)
backends-field-aliases = Алиасы (name=target по одному в строке)
backends-field-probe-models = Определять модели через проверочный запрос /models
backends-field-supports-edit = Поддерживает редактирование изображений
backends-status-saturated = перегружен
backends-auth-failed-title = Upstream отклонил учётные данные health-пробы (401/403), поэтому обнаружение моделей отключено — через этот бэкенд не станет доступно ничего нового. Проверьте ключ API; если он берётся из переменной окружения, проверьте, что она задана.
backends-auth-failed = ключ отклонён
backends-no-models-title = Этот бэкенд не объявляет ни одной модели, поэтому на него ничего не маршрутизируется, а голому псевдониму не к чему привязаться. Обычно это проба, которая ни разу не вернула данные.
backends-no-models = модели не заявлены
backends-key-env-badge = ключ: env { $var }
backends-key-env-unset-badge = env { $var } НЕ ЗАДАНА
backends-enabled-hint = Выключите, чтобы вывести бэкенд на обслуживание. Действует сразу — «Применить изменения» не нужно. Его модели остаются известными, поэтому нагрузку берут другие бэкенды пула, а клиенты видят временную недоступность, а не «модель не найдена».
backends-enabled-label = Принимает трафик
backends-activity-summary = 15м { $m15 } · 30м { $m30 } · 60м { $m60 }
backends-aliases-label = алиасы:
backends-alias-target-title = алиас → { $target }
backends-alias-disabled-title = простой алиас отключён — этот бэкенд обслуживает несколько моделей; укажите явную цель (форма сопоставления)
backends-alias-disabled-label = { $name } (отключён)
backends-fallback-offline-title = fallback_offline: используется, когда все бэкенды известной модели в этом пуле недоступны
backends-fallback-offline-badge = офлайн ↩ { $model }
backends-pool-empty = В этом пуле нет бэкендов.

backends-test-button = Проверить соединение
backends-test-hint = Обращается к этому URL с указанными выше учётными данными. Ничего не сохраняется.
backends-test-insert-hint = Объявленные id моделей — нажмите, чтобы дополнить строку псевдонима под курсором:
backends-test-ok = Доступен, аутентификация пройдена ({ $source }), найдено моделей: { $count }.
backends-test-ok-no-models = Доступен, аутентификация пройдена ({ $source }), но ответ не является конвертом OpenAI /models — обнаружение не может его прочитать. Этот бэкенд сможет обслуживать только id, перечисленные в поле «Модели».
backends-test-auth-failed = Отклонено с HTTP { $status }: учётные данные не приняты ({ $source }). Пока это не исправлено, обнаружение моделей отключено и бэкенд ничего не объявляет.
backends-test-http-error = { $url } ответил HTTP { $status }.
backends-test-unreachable = Не удалось связаться с { $url }: { $err }
backends-test-timeout = { $url } не ответил за { $secs } с.
backends-test-key-typed = с введённым выше ключом
backends-test-key-stored = с сохранённым ключом
backends-test-key-env = из env { $var }
backends-test-key-env-unset = env { $var } НЕ ЗАДАНА — запрос ушёл без учётных данных
backends-test-key-none = учётные данные не отправлены
backends-error-base-url-required = требуется базовый URL
