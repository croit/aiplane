# STATUS: llm-generated, unreviewed — pending native-speaker QA

tools-heading = Инструменты
tools-description = Включайте и выключайте инструменты, которые может использовать ассистент. Изменения применяются только к вашей учётной записи и вступят в силу со следующего сообщения.
tools-none-granted = Ваши роли не предоставляют доступ ни к одному инструменту.

tools-location-heading = Местоположение
tools-location-description = Поделитесь точным местоположением своего устройства, чтобы ассистент мог отвечать на вопросы вроде «какая здесь погода?». Оно используется только для вызовов инструментов, и вы можете отключить общий доступ в любой момент. Без него ассистент будет использовать приблизительное местоположение, определённое по вашему IP-адресу.
tools-location-share-button = Поделиться точным местоположением
tools-location-stop-button = Прекратить доступ
tools-location-shared = Доступ предоставлен.
tools-location-shared-accuracy = Доступ предоставлен — точность ±{ $accuracy } м.
tools-location-not-shared = Доступ не предоставлен.
tools-location-unavailable = Не удалось получить доступ к вашему местоположению. Проверьте разрешение браузера и повторите попытку.

# SPA-only: the Svelte /tools toggle list.
tools-toggle-aria = Переключить { $name }
tools-no-description = Нет описания

# Section headings for the tool catalog, shared by /tools, the per-token
# capability panel, the chat capability picker and the admin grant matrix.
tool-category-web-network = Веб и сеть
tool-category-attachments-documents = Вложения и документы
tool-category-document-templates = Шаблоны документов
tool-category-knowledge-base = База знаний
tool-category-code-sandbox = Код и песочница
tool-category-memory = Память
tool-category-integrations = Интеграции
tool-category-utility = Утилиты
tool-category-skills = Скиллы
tool-category-images-media = Изображения и медиа
tool-category-comfyui-workflows = Рабочие процессы ComfyUI
tool-category-scheduled-actions = Запланированные действия

tools-configure-link = Настроить
tools-needs-storage = Требуется файловое хранилище.
tools-needs-rag = Требуется база знаний.
tools-needs-push = Требуются push-уведомления.
tools-needs-geoip = Требуется база данных GeoIP.
tools-needs-image-backend = Требуется сервер генерации изображений.
tools-needs-sandbox = Требуется сервер для песочницы.
tools-needs-sandbox-network = Требуется доступ к сети из песочницы.

# SPA-only: /tools/browser, setting up the Chrome extension browser_control drives.
tools-browser-tab = Расширение браузера
tools-browser-description = Позвольте беседе действовать в этом браузере — с вашими входами в аккаунты — на страницах, доступных только вам: во внутреннем инструменте, на странице за единым входом, в форме, которую можете отправить только вы. Это работает, только пока беседа открыта, и только после того, как вы включите расширение.
tools-browser-status-heading = Состояние
tools-browser-status-not-granted = Ваши роли не разрешают управление браузером. Обратитесь к администратору, если оно вам нужно.
tools-browser-status-tool-off = Управление браузером выключено в вашем списке инструментов.
tools-browser-status-not-detected = На этой странице не ответило ни одно расширение. Установите его и добавьте этот AIplane в его настройках.
tools-browser-status-switched-off = Расширение установлено и связано, но выключено.
tools-browser-status-ready = Готово. Ассистент может действовать в этом браузере.
tools-browser-switch-on = Включить
tools-browser-switch-on-fallback = Chrome не открыл окно расширения. Нажмите на значок расширения на панели инструментов и выберите «Включить».
tools-browser-open-tools = Открыть список инструментов
tools-browser-recheck = Проверить снова
tools-browser-install-heading = Установка
tools-browser-store-button = Chrome Web Store
tools-browser-download-button = Скачать .zip
tools-browser-download-note = Архив .zip нужен для установки в распакованном виде: в Linux или в режиме разработчика Chrome. В Windows и macOS Chrome устанавливает расширения только из магазина.
tools-browser-steps-heading = Настройка
tools-browser-step-install = Установите расширение из Chrome Web Store. Нужен Chrome 127 или новее.
tools-browser-step-pair = Откройте настройки расширения и добавьте адрес этого AIplane. Chrome запросит разрешение для этого адреса; после разрешения связь установлена.
tools-browser-step-switch-on = Вернитесь на эту страницу и нажмите «Включить» или воспользуйтесь значком расширения на панели инструментов. В первый раз Chrome запросит доступ к сайтам.
tools-browser-step-ask = Попросите в беседе, например: «Открой нашу внутреннюю вики и кратко перескажи страницу об онбординге».
tools-browser-unpacked-heading = Установка в распакованном виде
tools-browser-unpacked-steps = Распакуйте архив, откройте chrome://extensions, включите режим разработчика, нажмите «Загрузить распакованное расширение» и выберите распакованную папку.
tools-browser-origin-label = Адрес этого AIplane
tools-browser-copy-origin = Копировать
tools-browser-copied = Скопировано
tools-browser-notes-heading = Полезно знать
tools-browser-note-window = Ассистент работает в собственном окне, в группе вкладок «Assistant». Пока расширение включено, его значок зелёный, а Chrome показывает панель о том, что браузер отлаживается.
tools-browser-note-open = Это работает, только пока беседа открыта во вкладке. Выключите расширение через его значок, когда закончите.
tools-browser-note-injection = Страницы, которые читает ассистент, могут содержать текст, обращённый к нему. В настройках расширения можно ограничить его одобренными вами сайтами.

tool-forward-request-title = Обратиться к другому Dot
tool-forward-request-description = Отправляет явную задачу другому личному Dot. Оба Dot должны разрешить запрос; принимающий Dot использует собственные инструменты и правила одобрения.
