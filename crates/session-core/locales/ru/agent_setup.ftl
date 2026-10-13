# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by the agent setup assistant (`/agents/{id}` Setup tab and
# `/agents/{id}/setup/{step}`): the overview, the steps, the checklist, and
# the starter templates' texts (`web/src/lib/agent-templates.json`).
agents-tab-setup = Настройка
agents-tab-try = Попробовать
agents-tab-insights = Аналитика
agents-tab-settings = Параметры
agents-setup-advanced = Расширенный редактор
agents-setup-advanced-hint = Для специалистов: форма, схема и JSON редактируют того же агента.
agents-setup-cta-title = Настроить шаг за шагом
agents-setup-cta-text = Помощник проведёт вас по всем пунктам. Позже каждый пункт можно открыть и отдельно.
agents-setup-cta-start = Запустить помощника
agents-setup-edit = Изменить
agents-setup-status-done = Готово
agents-setup-status-open = Не готово
agents-setup-status-optional = Необязательно
agents-setup-checklist = Перед публикацией
agents-setup-fix = Исправить
agents-setup-fix-advanced = Открыть расширенный редактор
agents-setup-ready = Всё готово. Можно публиковать.
agents-setup-ready-pill = Готов
agents-setup-open-count = { $count ->
    [one] { $count } пункт не готов
    [few] { $count } пункта не готовы
    [many] { $count } пунктов не готовы
   *[other] { $count } пункта не готовы
}
agents-setup-publish-blocked = Некоторые пункты ещё не готовы. См. «Перед публикацией» в разделе «Настройка».
agents-setup-check-scope = { $count ->
    [one] Список из { $count } темы
    [few] Список из { $count } тем
    [many] Список из { $count } тем
   *[other] Список из { $count } тем
}
agents-setup-check-scope-strict = { $count ->
    [one] Список из { $count } темы, строго соблюдается
    [few] Список из { $count } тем, строго соблюдается
    [many] Список из { $count } тем, строго соблюдается
   *[other] Список из { $count } тем, строго соблюдается
}
agents-setup-preview = Так это выглядит на вашем сайте
agents-setup-preview-offtopic = Как работает дизельный двигатель?
agents-setup-preview-free = Без строгого соблюдения модель решает сама и часто всё равно отвечает.
agents-setup-modal-note = Изменения сохраняются в черновик.
agents-setup-apply = Применить
agents-setup-assistant = Помощник
agents-setup-back = Назад
agents-setup-next = Далее
agents-setup-done = Готово
agents-setup-exit = Сохранить и выйти
agents-setup-step-of = Шаг { $current } из { $total }
agents-setup-unknown-step = Такого шага нет.
agents-setup-step-start = О чём речь?
agents-setup-step-start-short = Сценарий
agents-setup-step-basics = Задача и тон
agents-setup-step-basics-short = Задача
agents-setup-step-scope = Темы
agents-setup-step-scope-short = Темы
agents-setup-step-abilities = Знания и возможности
agents-setup-step-abilities-short = Знания
agents-setup-step-slots = Сбор данных
agents-setup-step-slots-short = Данные
agents-setup-step-identity = Проверка личности
agents-setup-step-identity-short = Личность
agents-setup-step-routes = Передача
agents-setup-step-routes-short = Передача
agents-setup-step-site = Сайт
agents-setup-step-site-short = Сайт
agents-setup-step-review = Проверка и тест
agents-setup-step-review-short = Проверка
agents-setup-start-lead = Опишите своими словами, что должен делать агент, или выберите шаблон.
agents-setup-scenario = Ваш сценарий
agents-setup-scenario-placeholder = Например: бот поддержки для нашего сайта, который отвечает на вопросы по документации и создаёт заявки.
agents-setup-propose = Предложить настройку
agents-setup-or-template = или выберите шаблон:
agents-setup-tpl-faq = FAQ для сайта
agents-setup-tpl-faq-desc = Отвечает на вопросы по вашим документам.
agents-setup-tpl-support = Поддержка клиентов с проверкой личности
agents-setup-tpl-support-desc = Помогает клиентам и проверяет, кто пишет, перед чувствительными темами.
agents-setup-tpl-leads = Квалификация лидов
agents-setup-tpl-leads-desc = Выясняет потребности и передаёт в отдел продаж.
agents-setup-tpl-internal = Внутренний помощник
agents-setup-tpl-internal-desc = Для сотрудников, с доступом к внутренним источникам.
agents-setup-tpl-blank = С чистого листа
agents-setup-tpl-blank-desc = Для специалистов, без предустановок.
agents-setup-tpl-replace = Заменить текущую настройку шаблоном «{ $template }»? Уже выданные доступы сохранятся.
agents-setup-tpl-applied = Шаблон применён. Пройдите шаги кнопкой «Далее» и уточните их.
agents-tpl-faq-task = Отвечай на вопросы посетителей по нашей документации. Если документация не охватывает вопрос, честно скажи об этом и не угадывай.
agents-tpl-faq-refusal = С этим я помочь не могу. С удовольствием отвечу на вопросы о наших продуктах и услугах.
agents-tpl-support-task = Помогай нашим клиентам с вопросами о продуктах и заказах. Прежде чем сообщать что-либо об учётной записи клиента, убедись, кто пишет. Если не можешь решить проблему, кратко опиши её и передай команде поддержки.
agents-tpl-support-refusal = К сожалению, с этим я помочь не могу. С удовольствием помогу с вопросами о наших продуктах и ваших заказах.
agents-tpl-leads-task = Выясни, что нужно посетителю: имя, адрес электронной почты, компанию и что он ищет. Отвечай на общие вопросы о нашем предложении, затем передай запрос отделу продаж.
agents-tpl-leads-refusal = С этим я помочь не могу. С удовольствием расскажу о нашем предложении.
agents-tpl-internal-task = Помогай сотрудникам находить информацию во внутренних источниках. Указывай источник каждого ответа.
agents-tpl-internal-refusal = Я помогаю только с вопросами по нашим внутренним темам.
agents-tpl-slot-name = Имя
agents-tpl-slot-email = Адрес электронной почты
agents-tpl-slot-company = Компания
agents-tpl-slot-need = Что ищет
agents-setup-name = Имя
agents-setup-task = Что должен делать агент?
agents-setup-task-hint = Одно-два предложения: для кого он и в чём помогает?
agents-setup-tone = Каким должен быть тон?
agents-setup-tone-friendly = Дружелюбный
agents-setup-tone-factual = Деловой
agents-setup-tone-casual = Непринуждённый
agents-setup-tone-brief = Кратко и по делу
agents-setup-tone-detailed = Подробный
agents-setup-tone-formal = На «вы»
agents-setup-tone-informal = На «ты»
agents-setup-language = Отвечает на
agents-setup-language-visitor = Языке посетителя
agents-setup-language-fixed = Всегда одном языке
agents-setup-language-none = Не задано
agents-setup-language-pick = Язык
agents-setup-tone-more = Ещё о том, как отвечать
agents-setup-tone-more-hint = Всё остальное о стиле или длине. Необязательно.
agents-setup-model = Модель
agents-setup-model-hint = Модель, которая отвечает за агента, — из того же списка, что и выбор модели в чате. «По умолчанию» следует модели чата по умолчанию шлюза.
agents-setup-model-default = По умолчанию ({ $model })
agents-setup-model-default-unheld = Модель по умолчанию { $model } вам самим недоступна, поэтому вы не можете дать её агенту. Выберите модель или обратитесь к администратору.
agents-setup-model-none = Нет модели, которую вы могли бы дать этому агенту. Попросите администратора о доступе к модели.
agents-setup-grant-failed = Не удалось выдать агенту доступ: { $reason }
agents-setup-scope-lead = О чём может говорить агент? На всё остальное он даёт ваш стандартный ответ.
agents-setup-topics = Разрешённые темы
agents-setup-topic-add = Добавить тему
agents-setup-topic-placeholder = Например: Счета
agents-setup-refusal = Ответ на другие темы
agents-setup-strict = Соблюдать строго (страж тем)
agents-setup-strict-hint = Перед каждым ответом небольшая модель проверяет, относится ли вопрос к темам. Если нет, посетитель получает стандартный ответ, а основная модель не вызывается.
agents-setup-strict-needs = Строгому стражу тем нужны хотя бы одна тема и ответ на другие темы.
agents-setup-scope-try = Попробуйте на вкладке «Попробовать»: тестовый чат показывает решение стража для каждого сообщения.
agents-setup-abilities-lead = Чем может пользоваться агент? Нужные доступы выдаются автоматически, насколько они есть у вас самих.
agents-setup-knowledge-no-search = Вам самим недоступен поиск по знаниям, поэтому вы не можете дать его агенту. Обратитесь к администратору.
agents-setup-locked = Выдано кем-то другим
agents-setup-locked-hint = У вас самих этого нет, поэтому изменить это вы не можете. Обратитесь к администратору.
agents-setup-kept-live = Доступ сохранён, потому что его использует опубликованная версия.
agents-setup-nothing-available = Пока нечего дать этому агенту. Администратор может подключить базы знаний и коннекторы.
agents-setup-abilities-more = Нет в списке? Администратор может подключить другие базы знаний и коннекторы.
agents-setup-slots-lead = Какие данные агент должен спросить в разговоре? Он спрашивает только тогда, когда они нужны.
agents-setup-slots-empty = Пока нечего собирать.
agents-setup-slot-label = Название
agents-setup-slot-kind = Вид
agents-setup-slot-kind-text = Текст
agents-setup-slot-kind-long_text = Длинный текст
agents-setup-slot-kind-email = Адрес электронной почты
agents-setup-slot-kind-phone = Номер телефона
agents-setup-slot-kind-customer_number = Номер клиента
agents-setup-slot-kind-order_number = Номер заказа
agents-setup-slot-kind-date = Дата
agents-setup-slot-kind-number = Число
agents-setup-slot-kind-yes_no = Да или нет
agents-setup-slot-kind-choice = Выбор из списка
agents-setup-slot-kind-custom = Настроено в расширенном редакторе
agents-setup-slot-values = Варианты через запятую
agents-setup-slot-add = Добавить поле
agents-setup-slot-new = Новое поле
agents-setup-slot-suggest = Предложения:
agents-setup-slot-in-use = Используется проверкой личности
agents-setup-identity-lead = Прежде чем показывать личные данные, агент должен знать, кто пишет. Как это проверять?
agents-setup-identity-none = Без проверки
agents-setup-identity-none-desc = Для общих вопросов. Личные данные остаются закрытыми.
agents-setup-identity-email_code = Код по электронной почте
agents-setup-identity-email_code-desc = Посетитель получает код от вашей системы и вводит его в защищённое поле.
agents-setup-identity-signed_in = Вход на вашем сайте
agents-setup-identity-signed_in-desc = Ваш сайт сообщает, кто вошёл в систему. Удобнее всего.
agents-setup-identity-customer_lookup = Номер клиента и имя
agents-setup-identity-customer_lookup-desc = Сверка с вашей системой. Слабее, для некритичных данных.
agents-setup-identity-connector = Какая система отправляет и проверяет код?
agents-setup-identity-connector-hint = Коннектор с инструментами send_code и check_code.
agents-setup-identity-no-connector = Вам не доступен ни один коннектор. Администратор может подключить его.
agents-setup-identity-tool = Какой инструмент проверяет клиента?
agents-setup-identity-tool-hint = Он получает имя и номер клиента и отвечает, совпадают ли они.
agents-setup-identity-dev = Для вашего веб-разработчика
agents-setup-identity-issuer = Издатель (адрес вашего сайта)
agents-setup-identity-audience = Аудитория (имя для этого агента)
agents-setup-identity-secret = Общий секрет
agents-setup-identity-secret-generate = Создать
agents-setup-identity-secret-hint = Не менее 32 символов. Ваш сайт подписывает им короткий токен; как именно, описано в документации по встраиванию.
agents-setup-identity-secret-set = Секрет сохранён. Введите новый, чтобы заменить его.
agents-setup-identity-blocked = «Без проверки» невозможно, пока эти передачи требуют подтверждённой личности: { $topic }.
agents-setup-identity-custom = { $count ->
    [one] Ещё { $count } проверка настроена в расширенном редакторе.
    [few] Ещё { $count } проверки настроены в расширенном редакторе.
    [many] Ещё { $count } проверок настроено в расширенном редакторе.
   *[other] Ещё { $count } проверки настроены в расширенном редакторе.
}
agents-setup-routes-lead = Когда агент должен передавать разговор специалисту или человеку? Читайте каждое правило как предложение.
agents-setup-rule-when = Если речь о
agents-setup-rule-topic = Тема
agents-setup-rule-topic-placeholder = Например: Счета
agents-setup-rule-and = и
agents-setup-rule-condition = Условие
agents-setup-rule-always = всегда
agents-setup-rule-verified = личность подтверждена
agents-setup-rule-details = все данные собраны
agents-setup-rule-both = все данные собраны и личность подтверждена
agents-setup-rule-then = , передать
agents-setup-rule-target = Кому передать
agents-setup-rule-person = человеку (входящие вашей команды)
agents-setup-rule-agent = Специалист: { $name }
agents-setup-rule-add = Добавить правило
agents-setup-fallback = Иначе, если агент не может помочь,
agents-setup-fallback-human = передать человеку
agents-setup-fallback-none = вежливо завершить разговор
agents-setup-routes-note = Специалист автоматически получает собранные данные, например подтверждённый номер клиента, но никогда не весь разговор.
agents-setup-notify = Сообщать о передаче человеку
agents-setup-notify-hint = Все, кто может ответить, увидят её во входящих. Также сообщать через:
agents-setup-notify-push = Push-уведомление всем, кто может ответить
agents-setup-notify-none = Пока нет канала для оповещений. Добавьте ниже канал Slack или Discord.
agents-setup-rule-needs-identity = Этот специалист работает с подтверждёнными данными клиента, поэтому правило требует подтверждённой личности.
agents-setup-rule-no-identity = Чтобы передавать только после подтверждения личности, сначала настройте проверку личности.
agents-setup-rule-no-details = Чтобы передавать разговор только после сбора всех данных, сначала добавьте их в разделе «Сбор данных».
agents-setup-rule-bind-missing = Специалисту нужно «{ $names }», а этот агент не собирает это в подтверждённом виде. Настройте это в расширенном редакторе.
agents-setup-rule-unreadable = Не удалось прочитать настройку специалиста. Возможно, у вас нет к ней доступа.
agents-setup-rule-no-target = У передачи «{ $topic }» ещё нет получателя.
agents-setup-routes-custom = { $count ->
    [one] Ещё { $count } правило настроено в расширенном редакторе.
    [few] Ещё { $count } правила настроены в расширенном редакторе.
    [many] Ещё { $count } правил настроено в расширенном редакторе.
   *[other] Ещё { $count } правила настроены в расширенном редакторе.
}
agents-setup-site-origins = На каком сайте появляется агент?
agents-setup-site-origins-hint = По одному адресу в строке, например https://www.example.com. Встраивать агента могут только эти сайты.
agents-setup-site-invalid = «{ $value }» — не адрес сайта. Пишите так: https://www.example.com.
agents-setup-site-code = Код для встраивания
agents-setup-site-code-hint = Создайте ключ для этих сайтов и вставьте строку прямо перед </body>. Ключ показывается только один раз.
agents-setup-site-create-key = Создать код для встраивания
agents-setup-site-keys = { $count ->
    [one] Активен { $count } ключ встраивания. Управление: Параметры → Доступ.
    [few] Активны { $count } ключа встраивания. Управление: Параметры → Доступ.
    [many] Активно { $count } ключей встраивания. Управление: Параметры → Доступ.
   *[other] Активны { $count } ключа встраивания. Управление: Параметры → Доступ.
}
agents-setup-copy = Копировать
agents-setup-copied = Скопировано
agents-setup-review-lead = Так ведёт себя { $name }:
agents-setup-review-try = Попробуйте на вкладке «Попробовать» и загляните за каждый ответ.
agents-setup-sum-basics = { $task } · Модель: { $model }
agents-setup-sum-no-task = Задача ещё не описана
agents-setup-sum-model-none = не выбрана
agents-setup-sum-scope-strict = { $topics }. Всё остальное отклоняет страж тем.
agents-setup-sum-scope-soft = { $topics }. Только как ориентир.
agents-setup-sum-scope-none = Списка тем пока нет
agents-setup-sum-abilities-none = Ничего не выбрано
agents-setup-sum-slots-none = Нечего собирать
agents-setup-sum-routes-other = Иначе → { $target }
agents-setup-sum-routes-none = Без передачи
agents-setup-sum-site-none = Сайт ещё не указан
agents-setup-todo-model = Выберите модель агента.
agents-setup-todo-task = Опишите, что должен делать агент.
agents-setup-todo-identity-connector = Выберите систему, которая отправляет код по почте.
agents-setup-todo-identity-tool = Выберите инструмент, который проверяет клиента.
agents-setup-todo-identity-token = Укажите издателя, аудиторию и общий секрет для проверки входа.
agents-setup-todo-site = Укажите сайт, на котором появляется агент, чтобы другие сайты не могли его встроить.
agents-setup-proposal-ready = Предложение готово. Каждый шаг теперь показывает свою часть — примените подходящее, остальное отклоните.
agents-setup-dropped = Не вошло:
agents-setup-suggest-dismiss = Отклонить
agents-setup-suggest-noted = Понятно
agents-error-network = Не удалось связаться с сервером. Проверьте подключение и попробуйте ещё раз.
agents-error-rate = Сейчас слишком много запросов. Подождите немного и попробуйте ещё раз.
agents-error-rate-retry = Сейчас слишком много запросов. Попробуйте ещё раз через { $seconds } с.
agents-error-assist-unavailable = Помощник сейчас недоступен на этом сервере. Попробуйте позже или обратитесь к администратору.
agents-error-assist-failed = Помощнику не удалось дать ответ на этот раз. Попробуйте ещё раз.
agents-error-generic = На сервере что-то пошло не так. Попробуйте ещё раз или обратитесь к администратору, если ошибка повторится.
agents-setup-suggest-task = Предложенная задача
agents-setup-suggest-tone = Предложенный тон
agents-setup-suggest-knowledge-missing = База знаний по теме «{ $topic }» ещё не подключена — попросите администратора добавить её.
agents-setup-suggest-tests = Предложенные тестовые разговоры
agents-setup-improve = Улучшить
agents-setup-improve-before = Было
agents-setup-improve-after = Предложение
agents-setup-test-kind-in = По теме
agents-setup-test-kind-out = Не по теме
agents-setup-test-save = Сохранить как тест
agents-setup-test-saved = Сохранено
agents-setup-slot-kind-whole_number = Целое число
agents-setup-site-color = Цвет
agents-setup-site-color-hint = Заголовок виджета, кнопка запуска и сообщения посетителей. Цвет текста подбирается так, чтобы он оставался читаемым.
agents-setup-site-color-clear = Цвет по умолчанию
agents-setup-voice = Голос
agents-setup-voice-hint = Посетители могут говорить вместо того, чтобы печатать, и слушать ответы. Записи не сохраняются; журнал активности хранит распознанный и прочитанный текст.
agents-setup-voice-input = Посетители могут надиктовать сообщение
agents-setup-voice-output = Ответы можно читать вслух
agents-setup-voice-transcription-model = Распознавание речи
agents-setup-voice-speech-model = Синтез речи
agents-setup-voice-voice = Голос (необязательно)
agents-setup-voice-voice-hint = Голоса, которые предлагает модель озвучивания.
agents-setup-voice-default = По умолчанию для языка посетителя
agents-setup-voice-unavailable-input = Голосовой ввод недоступен: для вас не включена ни одна модель распознавания речи — обратитесь к администратору.
agents-setup-voice-unavailable-output = Чтение ответов вслух недоступно: для вас не включена ни одна модель синтеза речи — обратитесь к администратору.

# The agent architect (#118): a centred chat window that plans an agent.
architect-open = Спланировать с архитектором
architect-title = Архитектор агентов
architect-intro = Планирует агента вместе с вами и создаёт черновик. Публикуете вы сами.
architect-conversation-title = Архитектор агентов: { $name }
architect-conversation-new = Архитектор агентов: новый агент
architect-tool-list-agents = Просмотрены агенты
architect-tool-read-agent = Прочитан агент
architect-tool-list-grantable = Проверены доступные права
architect-tool-propose-setup = Предложена настройка
architect-tool-create-agent-draft = Создан черновик
architect-tool-update-agent-draft = Изменён черновик
architect-tool-run-test-turn = Проведён тестовый диалог
architect-placeholder = Опишите, что должен делать ваш агент …
architect-send = Отправить
architect-new-conversation = Новый разговор
architect-undo = Отменить
architect-undone = Отменено
architect-open-setup = Открыть настройку
architect-starting = Архитектор запускается …
architect-empty = Расскажите архитектору, что должен делать агент, текстом или голосом. Каждое изменение видно здесь, и его можно отменить.
architect-create-option = Или спланируйте его в разговоре с архитектором
agents-setup-ready-recommended = Можно публиковать сейчас. Незавершённые пункты выше рекомендуются, но не обязательны.
agents-setup-ready-recommended-pill = Готов, есть рекомендации
agents-setup-test-save-all = Сохранить все как тесты
agents-setup-proposing = Настройка составляется по вашему описанию. Обычно это занимает от 20 до 40 секунд.
agents-setup-abilities-search = Поиск возможностей
dot-setup-title = Настроить Dot вместе
dot-setup-intro = Опишите задачи Dot. Мы проверим доступные ресурсы и зададим уточняющие вопросы. Dot будет создан только после подтверждения предложения.
dot-setup-empty = Например: Каждое утро подводить итоги работы компании по заявкам и письмам. Спрашивать меня перед внесением изменений.
dot-setup-placeholder = Что должен делать ваш Dot?
dot-setup-preview = Предложение по настройке Dot
dot-setup-identity = Идентификация
dot-setup-personal-identity = Личный Dot с вашими правами; подключённые учётные записи настраиваются отдельно.
dot-setup-details = Проверить рабочие инструкции
dot-setup-unresolved = Уточните эти требования в разговоре перед созданием Dot.
dot-setup-apply = Настроить Dot
architect-tool-propose-dot-setup = Подготовлено предложение по настройке Dot
dot-setup-error-conflict = Предложение изменилось. Проверьте текущее предложение перед настройкой Dot.
dot-setup-error-incomplete = Остались неуточнённые требования. Обсудите их в разговоре перед настройкой Dot.
dot-setup-error-unavailable = Разговор о настройке недоступен. Откройте настройку Dot заново и повторите попытку.
dot-setup-error-running = Ассистент ещё работает. Дождитесь ответа перед настройкой Dot.

dot-plan-unsupported = Этот параметр необходимо настроить вручную перед созданием Dot.
dot-plan-strict-scope-unavailable = Строгое ограничение тем недоступно при настройке личных Dots.
dot-plan-one-knowledge = Выберите одну коллекцию знаний для этой настройки.
dot-plan-task-required = Опишите задачи этого Dot.
dot-plan-connection-required = Для этого инструмента нужно сначала настроить подключение учётной записи.

dot-setup-tool-count = Инструменты: { $count }
