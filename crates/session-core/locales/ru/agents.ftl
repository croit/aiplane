# STATUS: llm-generated, unreviewed — pending native-speaker QA

agent-output-withheld = Я не могу дать этот ответ, так как в нём упоминались данные, которые я не смог подтвердить для вас. Переформулируйте вопрос или обратитесь в службу поддержки.
agent-output-redacted = [удалено]
agent-error-name-taken = В вашей учётной записи уже есть личный агент с таким именем. Выберите другое имя.
agent-error-name-invalid = Введите имя длиной не более 80 символов без управляющих символов.
agent-error-active-limit = Достигнут лимит активных личных агентов. Деактивируйте одного агента, прежде чем создавать следующего.
agent-error-not-found = Этот агент вам недоступен.
agent-error-id-missing = В запросе отсутствует идентификатор агента.
agent-memory-error-kind = Выберите для типа памяти: предпочтение, проект или факт.
agent-memory-error-empty = Содержимое памяти не может быть пустым.
agent-memory-error-id-missing = В запросе отсутствует идентификатор агента или записи памяти.

# What a website visitor reads next to the secure field when an identity
# verifier (#95) asks for the code it sent. Worded the same whether or not
# the address is registered, so it reveals neither.
agent-verifier-code-sent = Если этот адрес зарегистрирован у нас, мы отправили на него код. Введите код здесь.
agent-verifier-code-again = Введите код, который мы вам отправили.

# What a website visitor reads when the public agent endpoint refuses a
# request (#92): the agent's budget is spent, or a rate limit was hit.
agent-embed-unavailable = Этот ассистент временно недоступен. Попробуйте позже или воспользуйтесь другими способами связи на сайте.
agent-embed-rate-limited = Вы отправляете сообщения быстрее, чем этот ассистент их принимает. Подождите { $seconds } с и попробуйте снова.

agent-embed-decision-for-staff = Этот запрос ожидает решения нашего сотрудника. Ответ появится здесь, как только решение будет принято.
agent-embed-not-waiting = Сейчас ассистент не ждёт от вас ответа. Пожалуйста, перезагрузите разговор.
agent-embed-message-waiting = Ваше предыдущее сообщение всё ещё ждёт ассистента. Дождитесь ответа, прежде чем отправлять следующее.

agent-a2a-input-required = Партнёрскому сервису нужна ещё одна деталь, чтобы продолжить. Введите её здесь; она передаётся только этому сервису.
agent-error-inactive = Этот агент отключён. Включите его, прежде чем начинать диалог.
agent-action-chat = Чат
agent-action-chat-opening = Открываем чат…
agent-activity-session-started = Чат с агентом начат.

agent-workspace-error-unavailable = Постоянное хранилище агента недоступно. Попросите администратора проверить сервис sandbox.
agent-workspace-error-conflict = Рабочее пространство занято или изменилось. Обновите страницу и повторите попытку, когда агент не работает.
agent-workspace-error-not-found = Запись не найдена. Обновите каталог.
agent-workspace-error-too-large = Файл или каталог превышает лимит рабочего пространства. Используйте файл или каталог меньшего размера.
agent-workspace-error-invalid = Некорректный запрос. Проверьте путь и повторите попытку.
agent-workspace-error-failed = Операция не выполнена. Обновите страницу и повторите попытку.

agent-workspace-heading = Рабочая область
agent-workspace-intro = Файлы в этой рабочей области сохраняются между запусками агента. Скачайте файлы, чтобы просмотреть их.
agent-workspace-reload = Обновить рабочую область
agent-workspace-unavailable = Постоянное хранилище недоступно. Попросите администратора проверить сервис sandbox.
agent-workspace-busy = Этот агент использует свою рабочую область. Файлы могут изменяться. Загрузка файлов, создание папок и удаление доступны, когда агент не работает.
agent-workspace-empty = Эта папка пуста.
agent-workspace-root = Корень рабочей области
agent-workspace-folder = Папка
agent-workspace-size-missing = Размер недоступен
agent-workspace-modified = Изменено { $when }
agent-workspace-upload = Загрузить файл
agent-workspace-file = Файл
agent-workspace-upload-limit = Выберите файл размером не более 50 МиБ.
agent-workspace-create-folder = Создать папку
agent-workspace-folder-name = Имя папки
agent-workspace-delete = Удалить
agent-workspace-delete-confirm = Удалить «{ $name }»? Папки должны быть пустыми.
agent-workspace-more = Загрузить больше файлов
agent-workspace-error-load = Не удалось загрузить рабочую область. Обновите её и повторите попытку.
agent-workspace-error-save = Не удалось сохранить изменение. Обновите рабочую область и повторите попытку.
agent-workspace-notice-saved = Рабочая область обновлена.
