# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/mod.rs` — the
# multi-conversation chat page's server-side handlers: page title
# fallback, sidebar/effort/share/pin toasts, and the SSE-toast error
# messages the composer's fetch layer surfaces on failed actions.

chat-default-title = Chat

chat-error-auth-required = se requiere autenticación
chat-error-no-such-turn = no existe ese mensaje
chat-error-db-error = error de base de datos
chat-error-attachments-not-configured = los adjuntos del chat no están configurados
chat-error-bad-filename = nombre de archivo no válido
chat-error-attachment-not-found = no encontrado

# SPA-only chat chrome (`web/src/routes/chat/*`): the conversation list,
# the conversation header, the assistant's ask-back card, and the
# turn-status line the server never renders itself.
chat-list-empty = Aún no hay conversaciones. Empieza una arriba.
chat-turn-stopped = detenido
chat-prompt-heading = El asistente pregunta
chat-prompt-placeholder = Escribe una respuesta…
chat-prompt-answer = Responder
chat-prompt-skip = Omitir

# El campo de entrada sigue disponible mientras se genera una respuesta.
chat-queue-label = { $count ->
    [one] { $count } mensaje en espera
   *[other] { $count } mensajes en espera
  }
chat-queue-move-up = Subir
chat-queue-move-down = Bajar
chat-queue-edit = Devolver al campo de entrada
chat-queue-remove = Descartar
chat-queue-held-title = El archivo adjunto se perdió al recargar la página — vuelve a adjuntarlo o descarta el mensaje
chat-queue-held-hint = Un mensaje en espera perdió su archivo adjunto al recargar la página. No se enviará hasta que lo devuelvas al campo de entrada y adjuntes el archivo de nuevo.
chat-queue-was-interjection = Escrito durante la respuesta anterior, que terminó antes de leerlo
chat-composer-interject = Añadir a la respuesta en curso
chat-composer-interject-title = Se entrega a la respuesta que se está escribiendo (Ctrl/Cmd+Intro). Llega en el siguiente paso de herramienta; si la respuesta termina antes, se envía como mensaje siguiente.
chat-composer-interrupt = Interrumpir y reorientar
chat-composer-interrupt-title = Detener la respuesta actual y enviar esto en su lugar. Lo ya escrito permanece en la conversación.
chat-steer-pending = Añadido durante esta respuesta — aún sin leer
chat-steer-delivered = Añadido durante esta respuesta — tenido en cuenta
chat-steer-resent = Añadido durante esta respuesta — llegó tarde, enviado como mensaje siguiente
chat-steer-discarded = Añadido durante esta respuesta — llegó tarde y se descartó
