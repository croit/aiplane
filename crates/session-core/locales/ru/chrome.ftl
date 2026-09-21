# STATUS: llm-generated, unreviewed — pending native-speaker QA

chrome-theme-toggle-title = Переключить тему
chrome-theme-toggle-aria-to-light = Переключить на светлую тему
chrome-theme-toggle-aria-to-dark = Переключить на тёмную тему
chrome-lang-switcher-aria = Выбрать язык

# Web Push turn-complete notifications (server-sent body; `spawn_assistant_worker`).
push-untitled-conversation = Новый разговор
push-turn-complete-body = Ваш ответ готов.
push-turn-error-body = Запрос завершился ошибкой.

# Web Push: авторизация коннектора перестала действовать (фоновое обновление,
# `tools::mcp::worker`). { $connector } — отображаемое имя коннектора.
push-connector-reconnect-title = Подключение требует входа
push-connector-reconnect-body = { $connector } отключён — откройте «Интеграции», чтобы подключить заново.

# SPA-only: the root route, which only routes on to /chat.
chrome-opening-conversations = Открываем ваши диалоги…

# SPA-only: /login, which exists to bounce straight to the IdP.

searchable-select-search-placeholder = Поиск вариантов…
searchable-select-search-aria = Поиск: { $field }
searchable-select-clear-search = Очистить поиск
searchable-select-no-results = Подходящих вариантов нет.
searchable-select-model-gdpr = GDPR
searchable-select-model-nda = NDA

# Показывается вместо страницы, чья необязательная функция отключена в
# /admin/settings. Пункт навигации при этом исчезает; это ответ тому, кто
# перешёл по старой ссылке или ввёл URL вручную.
feature-disabled-body = { $feature } отключено на этом шлюзе, поэтому странице нечего показать. Администратор может включить это в настройках.
feature-disabled-settings-link = Открыть настройки

multi-select-none = Ничего не выбрано
multi-select-count = Выбрано: { $count }
multi-select-clear = Очистить всё
multi-select-unknown = Здесь не зарегистрировано — удалите или создайте
multi-select-wildcard-tools = Все инструменты, включая добавленные позже
multi-select-wildcard-skills = Все навыки, включая добавленные позже
multi-select-shadowed = Покрыто ✱
multi-select-wildcard-warning = ✱ также без проверки выдаёт инструменты, которые появятся в будущих выпусках. Для группы без прав администратора лучше перечислить нужные инструменты явно.
