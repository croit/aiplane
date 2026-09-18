# STATUS: llm-generated, unreviewed — pending native-speaker QA


pools-field-name = Название
pools-field-kind = Тип
pools-field-models = Обслуживаемые модели (белый список, через запятую)
pools-field-backends = Бэкенды
pools-save-pool = Сохранить пул
pools-delete-pool = Удалить

# Карточки пулов в SPA: две однострочные сводки и подтверждение удаления.
pools-add-heading = Добавить пул
pools-fallbacks-heading = Резервы для неизвестных моделей
pools-fallbacks-description = Когда в запросе указана модель, о которой шлюз никогда не слышал, подставлять эту модель для данного типа. Пусто = промах возвращает 404.
upstreams-problems-heading = Этот пул работает не полностью
upstreams-coverage-heading = Что видят клиенты
upstreams-coverage-hint = Ровно те имена, которые GET /v1/models возвращает для этого пула, с числом бэкендов, способных обслужить каждое прямо сейчас. Всё, что меньше полного, означает, что для этого имени часть вашего железа простаивает.
upstreams-coverage-none-title = Сейчас ни один бэкенд не может обслужить это имя. Запросы получат ошибку временной недоступности.
upstreams-coverage-partial-title = Это имя обслуживает лишь часть бэкендов — запросы используют часть пула, остальное простаивает. Обычно это псевдоним, чья цель не совпадает с тем, что объявляет бэкенд.
upstreams-coverage-full-title = Все бэкенды пула обслуживают это имя.
pools-name-taken = Пул с таким именем уже существует — сохранение заменит его вместе с бэкендами и моделями. Выберите другое имя, чтобы создать новый пул.
pools-field-strategy = Стратегия
pools-field-strategy-hint = prefix_affinity удерживает диалог на той реплике, у которой уже есть его KV-кэш (лучший выбор для чат-/агентского трафика на нескольких GPU — остальные стратегии отправляют соседние ходы на разные реплики и каждый раз платят полный prefill), и всё равно распределяет нагрузку, если бэкенд действительно занят сильнее. least_inflight балансирует по текущей загрузке; round_robin вращается с учётом веса.
pools-field-fallback-offline = Резервная офлайн-модель
pools-field-fallback-offline-placeholder = используется, когда все бэкенды недоступны
pools-field-models-hint = Если задано, от бэкенда с зондированием /models обслуживаются только эти id — остальные показаны зачёркнутыми. Пусто = обслуживать всё, что сообщает бэкенд.
pools-field-allowed-groups = Разрешённые группы
pools-field-allowed-groups-hint = Группы AIplane (через запятую), которым разрешено видеть и использовать модели этого пула. Пусто = все. Админы всегда имеют доступ. Управление группами в Admin → Группы.
pools-field-voices = Голоса (lang=voice по одному в строке)
pools-field-offer-voices = Доступные голоса (по одному в строке, выбирает пользователь)
pools-no-backends = Бэкенды ещё не заданы. Сначала добавьте один на странице «Бэкенды».
pools-field-gdpr = Соответствует GDPR
pools-field-nda = Покрыто NDA
pools-field-enforce-limits = Применять лимиты запросов и квоты

upstreams-problem-no-backends = Бэкенды не назначены — в этом пуле некому обслуживать запросы.
upstreams-problem-all-drained = Все бэкенды выведены на обслуживание, сюда ничего не маршрутизируется.
upstreams-problem-all-down = Нет доступных бэкендов: запросы ждут возврата, а затем получают ошибку временной недоступности.
upstreams-problem-auth = Учётные данные отклонены: { $backends }. Там обнаружение моделей отключено, и эти бэкенды ничего не объявляют.
upstreams-problem-no-models = Не объявляют моделей: { $backends }. К ним ничего не маршрутизируется, а голому псевдониму там не к чему привязаться.
upstreams-problem-broken-aliases = Псевдонимы, ведущие в никуда: { $aliases }. Каждый указывает на модель, которую его бэкенд не обслуживает, либо ему не к чему привязаться.
upstreams-problem-partial-coverage = Обслуживаются лишь частью пула: { $models }. Запросы к этим именам используют меньше реплик, чем у вас есть.
upstreams-problem-unserved-allowlist = Указаны в «Модели», но никем не обслуживаются: { $models }.
upstreams-problem-missing-backends = Назначенные бэкенды, которых больше нет: { $backends }.
upstreams-apply-diff-summary = Показать, что изменит применение
upstreams-diff-pool-added = новый пул { $pool } начинает обслуживание
upstreams-diff-pool-removed = пул { $pool } прекращает обслуживание
upstreams-diff-pool-kind = пул { $pool }: тип { $from } → { $to }
upstreams-diff-pool-strategy = пул { $pool }: стратегия { $from } → { $to }
upstreams-diff-backend-joins = { $backend } входит в пул { $pool } и начинает принимать трафик
upstreams-diff-backend-leaves = { $backend } выходит из пула { $pool } и прекращает принимать трафик
upstreams-diff-backend-url = { $backend }: базовый URL { $from } → { $to } (обнаруженные модели будут опрошены заново)
upstreams-diff-backend-limits = { $backend }: вес { $weight }, макс. параллельно { $inflight }
upstreams-diff-backend-health-path = { $backend }: health-путь → { $to }
