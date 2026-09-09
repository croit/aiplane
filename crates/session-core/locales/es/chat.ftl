# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/mod.rs` — the
# multi-conversation chat page's server-side handlers: page title
# fallback, sidebar/effort/share/pin toasts, and the SSE-toast error
# messages the composer's fetch layer surfaces on failed actions.

chat-default-title = Chat

chat-error-still-streaming = Todavía se está transmitiendo una respuesta para este usuario — espera o pulsa Detener.

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
chat-list-pinned-badge = fijada
chat-list-pin = Fijar
chat-list-unpin = Desfijar
chat-list-delete = Eliminar
chat-push-invite = Recibe un aviso cuando termine una respuesta.
chat-all-chats = Todos los chats
chat-turn-stopped = detenido
chat-prompt-heading = El asistente pregunta
chat-prompt-placeholder = Escribe una respuesta…
chat-prompt-answer = Responder
chat-prompt-skip = Omitir
