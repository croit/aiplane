# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `session-core/src/render.rs` — the HTML renderers for
# the chat-style session UI (conversation bubbles, tool-call rows, the
# document canvas, and the composer). Driver-agnostic: both the gateway
# and any future consumer of this crate render through these functions.

render-edit-button = ✎ Editar

render-retry-button = ↻ Reintentar

render-attachment-remove-aria = Eliminar adjunto

# Botón de copiar en un bloque de código de una respuesta (solo icono,
# por lo que este es su tooltip / nombre accesible).

render-thinking-spinner = Pensando…
render-thinking-finalized = Pensó durante { $secs } s

render-tool-status-used = Utilizada

render-canvas-edit-button = ✎ Editar
render-canvas-save = Guardar como nueva versión
render-canvas-cancel = Cancelar

render-composer-attach-aria = Adjuntar archivos
render-composer-attach-title = Adjuntar archivos (también arrastrar / pegar)
render-composer-send = Enviar
render-composer-stop = Detener

# The browser prompt behind the ✎ Edit button, and the per-file title on
# an attachment's remove button.
render-edit-prompt = Edita tu mensaje:
render-attachment-remove-title = Quitar { $filename }
