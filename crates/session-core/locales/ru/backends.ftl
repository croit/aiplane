# STATUS: llm-generated, unreviewed — pending native-speaker QA

backends-page-title = Восходящие бэкенды — LLM Gateway
backends-heading = Восходящие бэкенды
backends-description-prefix = Живой обзор настроенных восходящих пулов — состояние, текущая нагрузка относительно лимита каждого бэкенда и модели, которые каждый из них сейчас предоставляет. Только для чтения: маршрутизация полностью зависит от того, что бэкенды сообщают через свой
backends-description-suffix = проверочный запрос.
backends-summary = { $total } бэкендов · { $healthy } исправны · { $down } недоступны
backends-unknown-fallback-prefix = Резерв для неизвестной модели —
backends-empty-prefix = Восходящие пулы не настроены. Добавьте блок
backends-empty-suffix = в gateway.toml и перезапустите.

backends-fallback-offline-title = fallback_offline: используется, когда все бэкенды известной модели в этом пуле недоступны
backends-fallback-offline-badge = офлайн ↩ { $model }
backends-pool-empty = В этом пуле нет бэкендов.

backends-status-down = недоступен
backends-status-saturated = перегружен
backends-status-up = активен

backends-inflight-label = в обработке { $load }
backends-activity-summary = 15м { $m15 } · 30м { $m30 } · 60м { $m60 }
backends-no-models = модели не заявлены
backends-aliases-label = алиасы:

backends-alias-target-title = алиас → { $target }
backends-alias-disabled-label = { $name } (отключён)
backends-alias-disabled-title = простой алиас отключён — этот бэкенд обслуживает несколько моделей; укажите явную цель (форма сопоставления)
backends-alias-bare-title = алиас → модель этого бэкенда

# Backend CRUD editor (add/edit/delete backends stored in the DB topology).
backends-manage-heading = Управление бэкендами
backends-manage-description = Добавляйте, редактируйте или удаляйте восходящие бэкенды. Изменения сохраняются в базе данных, но вступают в силу только после нажатия «Применить изменения».
backends-apply-changes = Применить изменения
backends-add-heading = Добавить бэкенд
backends-field-name = Название
backends-field-base-url = Базовый URL
backends-field-api-key-env = Переменная окружения с API-ключом
backends-field-health-path = Путь проверки состояния
backends-field-weight = Вес
backends-field-max-inflight = Макс. одновременных
backends-field-pool = Пул
backends-field-pool-none = (нет)
backends-field-pool-hint = Назначает этот бэкенд одному пулу. Бэкенд в нескольких пулах сводится к выбранному здесь.
backends-field-models = Модели (через запятую)
backends-field-aliases = Алиасы (name=target по одному в строке)
backends-field-probe-models = Определять модели через проверочный запрос /models
backends-field-supports-edit = Поддерживает редактирование изображений
backends-save-backend = Сохранить бэкенд
backends-add-backend = Добавить бэкенд
backends-delete-backend = Удалить
backends-error-name-required = требуется название бэкенда
backends-error-base-url-required = требуется базовый URL
backends-saved = бэкенд `{ $name }` сохранён — нажмите «Применить изменения» для перезагрузки
backends-deleted = бэкенд `{ $name }` удалён — нажмите «Применить изменения» для перезагрузки

backends-field-api-key = Ключ API
backends-field-api-key-placeholder = Ключ API (хранится в зашифрованном виде)
backends-field-api-key-keep = оставьте пустым, чтобы сохранить текущий ключ

# Duplicate-name guard on the Add-backend form.
backends-error-name-exists = бэкенд с именем `{ $name }` уже существует — нажмите «Добавить бэкенд» ещё раз, чтобы перезаписать его, или измените имя
backends-overwrite-hint = Такое имя уже существует. Повторное сохранение ПЕРЕЗАПИШЕТ существующий бэкенд — его базовый URL, ключ API, модели, псевдонимы и привязку к пулу. Измените имя, чтобы добавить второй бэкенд.

# An alias that is configured but would not route.
backends-alias-unresolved-title = СЛОМАН: этот псевдоним указывает на `{ $target }`, который этот бэкенд не обслуживает — запросы к нему завершаются ошибкой.
backends-alias-nothing-title = СЛОМАН: этот бэкенд не объявляет ни одной модели, поэтому голому псевдониму не к чему привязаться.
backends-alias-serves = Обслуживается: { $models }
backends-alias-serves-nothing = Сейчас не обслуживается ни одна модель.
# Save-time check on the aliases textarea.
backends-alias-target-unknown = сохранено, но эти цели псевдонимов не обслуживаются этим бэкендом: { $targets } — обслуживается { $models }. Такие псевдонимы не будут маршрутизироваться, пока цель не совпадёт в точности.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
backends-auth-failed = ключ отклонён
backends-auth-failed-title = Upstream отклонил учётные данные health-пробы (401/403), поэтому обнаружение моделей отключено — через этот бэкенд не станет доступно ничего нового. Проверьте ключ API; если он берётся из переменной окружения, проверьте, что она задана.
backends-no-models-title = Этот бэкенд не объявляет ни одной модели, поэтому на него ничего не маршрутизируется, а голому псевдониму не к чему привязаться. Обычно это проба, которая ни разу не вернула данные.
# Maintenance switch.
backends-enabled-label = Принимает трафик
backends-enabled-hint = Выключите, чтобы вывести бэкенд на обслуживание. Действует сразу — «Применить изменения» не нужно. Его модели остаются известными, поэтому нагрузку берут другие бэкенды пула, а клиенты видят временную недоступность, а не «модель не найдена».
backends-enabled-on = бэкенд `{ $name }` снова принимает трафик
backends-enabled-off = бэкенд `{ $name }` выведен на обслуживание — новые запросы к нему не маршрутизируются
backends-status-drained = обслуживание
backends-status-drained-title = Выведен на обслуживание: бэкенд доступен, но маршрутизатор его пропускает. Включите «Принимает трафик», чтобы вернуть его в ротацию.
# "Test connection": call the upstream with what is typed in the editor.
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
# Where the API key comes from (U5).
backends-key-env-badge = ключ: env { $var }
backends-key-env-title = У этого бэкенда нет сохранённого ключа; он читает его из этой переменной окружения, которая сейчас задана.
backends-key-env-unset-badge = env { $var } НЕ ЗАДАНА
backends-key-env-unset-title = У этого бэкенда нет сохранённого ключа, а указанная переменная окружения в процессе шлюза не задана — учётные данные вообще не отправляются. Если upstream их требует, каждая проба получает 401, обнаружение моделей остаётся отключённым и бэкенд ничего не объявляет. Введите ключ в поле «Ключ API» либо задайте переменную и перезапустите.
# Live name-clash note on the add form (U9).
backends-name-taken = Бэкенд с таким именем уже существует — сохранение перезапишет его вместе с базовым URL, ключом, моделями и пулом. Выберите другое имя, чтобы добавить второй бэкенд.
