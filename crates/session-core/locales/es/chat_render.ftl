# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/render.rs` — the
# gateway-only chat-page chrome: the header model/voice pickers, the
# compliance banners, the composer's "+" tools/integrations/skills menu,
# the "Denken" (effort/thinking) picker, and the share/export/fork
# controls. Prefixed `chat-render-` (rather than `chat-`) to avoid
# colliding with `chat/mod.rs`'s own `chat-*` keys in the sibling
# `chat.ftl`.

chat-render-model-placeholder = modelo (p. ej., gpt-4o-mini)
chat-render-model-aria = Modelo de chat

chat-render-composer-placeholder = Escribe un mensaje al modelo…

chat-render-effort-title = Esfuerzo de razonamiento
chat-render-effort-tooltip = Esfuerzo de razonamiento: más alto = más razonamiento y más rondas de herramientas, pero más lento
chat-render-effort-label-prefix = Razonamiento:
chat-render-effort-fast = Rápido
chat-render-effort-standard = Estándar
chat-render-effort-deep = Profundo
chat-render-effort-max = Máximo

chat-render-tools-tooltip = Herramientas, integraciones y skills para esta conversación
chat-render-tools-label = Herramientas

chat-render-close = Cerrar

chat-render-share-label-on = Compartido ✓
chat-render-share-label-off = Compartir
chat-render-share-tooltip = Los chats compartidos pueden leerlos cualquier usuario con sesión iniciada que tenga el enlace

chat-render-fork-tooltip = Copia esta conversación en tus propios chats para seguir chateando
chat-render-fork-label = Continuar en mis chats

chat-render-export-tooltip = Descargar esta conversación
chat-render-export-aria = Exportar conversación
chat-render-export-label = Exportar
chat-render-export-pdf = Documento PDF
chat-render-export-md = Markdown (.md)

# SPA-only chat-page chrome (`web/src/routes/chat/[id]`): the canvas
# document list and its save state, the composer's per-tool remove
# button, the model picker's compliance tooltips, and the attachment
# size chip.
chat-render-documents-label = Documentos
chat-render-revision-count = { $count ->
    [one] { $count } revisión
   *[other] { $count } revisiones
}
chat-render-canvas-saving = Guardando…
chat-render-tool-disable-aria = Desactivar { $name }
chat-render-model-gdpr-region = Región RGPD
chat-render-model-nda-covered = Cubierto por NDA
chat-render-attachment-size-kb = { $size } KB
