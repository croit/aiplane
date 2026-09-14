# STATUS: llm-generated, unreviewed — pending native-speaker QA

memory-heading = Память
memory-description = Что ассистент помнит о вас, сгруппировано по типу. Добавляйте, изменяйте или удаляйте записи здесь — это память вашей учётной записи, полностью под вашим контролем. Саму функцию можно включить или выключить на странице «Инструменты».

memory-add-heading = Добавить запись
memory-kind-aria = Тип записи
memory-content-placeholder = напр. Предпочитает ответы в метрических единицах

memory-empty = Здесь пока ничего нет.
memory-save-button = Сохранить
memory-delete-title = Удалить запись

# SPA-only: the Svelte /memory page's inline add/edit form.
memory-kind-preference = Предпочтения
memory-kind-project = Контекст проекта
memory-kind-fact = Факты
memory-content-label = Содержимое
memory-add-button = Запомнить
memory-delete-confirm = Удалить эту запись?

# The per-category Add button in each card header; it opens the add dialog
# with that card's kind already selected.
memory-add-short = Добавить

# One line under each card heading saying how that kind reaches the
# assistant: preferences ride in the system context of every conversation,
# while project notes and facts wait to be looked up with `recall`. The
# difference changes which bucket a user files something in, and nothing
# else on the page reveals it.
memory-kind-preference-hint = Всегда в контексте — передаются с первого сообщения каждого разговора, поэтому ассистент следует им без напоминаний.
memory-kind-project-hint = Запрашивается по необходимости — ассистент обращается к этим записям, когда разговор касается вашей работы.
memory-kind-fact-hint = Запрашивается по необходимости — ассистент обращается к этим записям, как только они становятся важны.
