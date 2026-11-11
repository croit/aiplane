# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Поверхность апстримов: что такое пул и заголовки табло.

upstreams-heading = Апстримы
upstreams-description = Пулы группируют бэкенды по виду и стратегии выбора. Здоровье, нагрузка и обслуживаемые модели проверяются в реальном времени. Изменения топологии сохраняются в базе и сразу применяются в работающем реестре.
upstreams-add-pool = Пул
upstreams-add-backend = Бэкенд
upstreams-unassigned-heading = Без назначения
upstreams-unassigned-description = Бэкенды, не назначенные ни одному пулу. Добавьте бэкенд в пул, чтобы направлять на него трафик.
upstreams-empty = Пулы и бэкенды ещё не настроены. Добавьте пул или бэкенд, чтобы начать.
upstreams-cancel = Отмена
upstreams-edit-backend = Изменить бэкенд
upstreams-comp-limits = лимиты
upstreams-edit-pool = Изменить пул

# Табло перрона: заголовки столбцов, словарь огней, действия строк.
board-col-name = Бэкенд
board-col-state = Состояние
board-col-load = Нагрузка
board-col-requests = Запросов/ч
board-summary-on-time = вовремя
board-summary-degraded = деградация
board-summary-down = вне сети
board-summary-maintenance = обслуживание
board-summary-hint = Один счёт на бэкенд, по всем его пулам.
board-on-time = Вовремя
board-on-time-detail = Здоров, обслуживает трафик
board-saturated = Насыщен
board-saturated-detail = { $inflight } из { $max } рабочих запросов занято
board-down = Вне сети
board-down-detail = Проведка здоровья не проходит
board-maintenance = Обслуживание
board-maintenance-detail = На паузе — не принимает новый трафик
board-auth = Ключ отклонён
board-auth-detail = Апстрим отклонил API-ключ
board-no-probe = Без проводки
board-no-probe-detail = Ещё не проверено
board-models-unit = моделей
board-legend-on-time = вовремя — здоров, обслуживает
board-legend-degraded = задержка — насыщен или проблема с ключом
board-legend-down = отменено — недоступен
board-legend-maintenance = обслуживание — на паузе
board-enabled = Обслуживает
board-drained = На паузе
board-edit = Изменить

board-detail-endpoint = Эндпоинт
board-detail-models = Модели
board-detail-aliases = Алиасы
board-detail-activity = Трафик
board-key-none = ключ не настроен
board-models-none = ничего не обнаружено
board-models-more = +{ $count } ещё
board-pool-account = Аккаунт
board-pool-managed = Этот пул управляется на вкладке OpenRouter.

board-filter-placeholder = Фильтр по бэкендам, моделям, алиасам…
board-filter-clear = Сбросить
board-filter-empty = Ни один бэкенд и ни один пул не подходят под этот фильтр.
board-edit-named = Изменить бэкенд { $name }
upstreams-saved = Сохранено — уже действует.
board-model-open-catalog = Открыть эту модель в каталоге
