# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/render.rs` — the
# gateway-only chat-page chrome: the header model/voice pickers, the
# compliance banners, the composer's "+" tools/integrations/skills menu,
# the "Denken" (effort/thinking) picker, and the share/export/fork
# controls. Prefixed `chat-render-` (rather than `chat-`) to avoid
# colliding with `chat/mod.rs`'s own `chat-*` keys in the sibling
# `chat.ftl`.

chat-render-model-placeholder = модель (напр., gpt-4o-mini)
chat-render-model-aria = Модель чата

chat-render-composer-placeholder = Сообщение модели…

chat-render-effort-title = Уровень размышлений
chat-render-effort-tooltip = Уровень размышлений: выше = больше рассуждений и больше циклов инструментов, но медленнее
chat-render-effort-label-prefix = Размышления:
chat-render-effort-fast = Быстро
chat-render-effort-standard = Стандарт
chat-render-effort-deep = Глубоко
chat-render-effort-max = Максимум

chat-render-tools-tooltip = Инструменты, интеграции и скиллы для этой беседы
chat-render-tools-label = Инструменты

chat-render-close = Закрыть

chat-render-share-label-on = Открыт доступ ✓
chat-render-share-label-off = Поделиться
chat-render-share-tooltip = Общие чаты может читать любой авторизованный пользователь, у которого есть ссылка

chat-render-fork-tooltip = Скопировать эту беседу в свои чаты, чтобы продолжить общение
chat-render-fork-label = Продолжить в моих чатах

chat-render-export-tooltip = Скачать эту беседу
chat-render-export-aria = Экспортировать беседу
chat-render-export-label = Экспорт
chat-render-export-pdf = PDF-документ
chat-render-export-md = Markdown (.md)

# SPA-only chat-page chrome (`web/src/routes/chat/[id]`): the canvas
# document list and its save state, the composer's per-tool remove
# button, the model picker's compliance tooltips, and the attachment
# size chip.
chat-render-documents-label = Документы
chat-render-revision-count = { $count ->
    [one] { $count } версия
    [few] { $count } версии
   *[many] { $count } версий
}
chat-render-canvas-saving = Сохранение…
chat-render-tool-disable-aria = Отключить { $name }
chat-render-model-gdpr-region = Регион GDPR
chat-render-model-nda-covered = Покрыто NDA
chat-render-attachment-size-kb = { $size } КБ
