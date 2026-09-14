# STATUS: llm-generated, unreviewed — pending native-speaker QA


webhooks-heading = Webhooks
webhooks-intro = Ejecuta un prompt cuando un servicio externo llame a una URL. Obtienes una URL de activación secreta; lo que el llamante envíe en el cuerpo de la solicitud se añade a tu prompt, y la ejecución se abre como un nuevo chat que puedes leer aquí.
webhooks-create-submit = Crear webhook
webhooks-save-submit = Guardar cambios
webhooks-edit-heading = Editar webhook
webhooks-back = Atrás
webhooks-list-heading = Tus webhooks
webhooks-list-empty = Aún no hay webhooks. Crea uno con el botón de arriba.

webhooks-name-label = Nombre
webhooks-name-placeholder = p. ej. Resumen de despliegue
webhooks-model-label = Modelo
webhooks-model-placeholder = ID del modelo
webhooks-prompt-label = Prompt
webhooks-prompt-placeholder = ¿Qué debe hacer el modelo con los datos entrantes?

webhooks-sync-toggle-label = Esperar la respuesta (devolver la salida del modelo al llamante)
webhooks-tools-toggle-label = Permitir herramientas (ejecutar con tus herramientas, p. ej. búsqueda web, RAG, conectores)
webhooks-tools-warning = Cualquiera con la URL de activación puede enviar contenido que el modelo procesa con tus herramientas, actuando como tú. Actívalo solo para un llamante de confianza.

webhooks-gdpr-warning = Este modelo se ejecuta fuera de la UE. No envíes datos personales a través de este webhook.
webhooks-nda-warning = Este modelo no está autorizado para contenido restringido por NDA. No envíes datos confidenciales a través de este webhook.

webhooks-reveal-heading = Tu URL de activación
webhooks-reveal-note = Cópiala ahora — solo se muestra una vez. Cualquiera con esta URL puede activar el webhook. ¿La perdiste? Rótala para obtener una nueva.
webhooks-copy = Copiar

webhooks-badge-active = Activo
webhooks-badge-paused = En pausa
webhooks-mode-sync = Espera la respuesta
webhooks-mode-async = Disparar y olvidar
webhooks-never-fired = Nunca se ha activado
webhooks-last-success = Última activación { $when }
webhooks-last-failure = Última activación fallida { $when }

webhooks-pause-title = Pausar
webhooks-resume-title = Reanudar
webhooks-rotate-title = Rotar secreto
webhooks-edit-title = Editar
webhooks-delete-title = Eliminar


webhooks-toast-not-found = Webhook no encontrado.

# --- Reejecutar con un prompt diferente ---
webhooks-rerun-link = reejecutar
webhooks-rerun-heading = Reejecutar con un prompt diferente
webhooks-rerun-page-name = Reejecutar webhook
webhooks-rerun-intro = Reproduce la última carga útil que recibió este webhook, con un prompt que puedes editar. La ejecución se abre como un nuevo chat.
webhooks-rerun-payload-label = Carga útil capturada (se reproduce tal cual)
webhooks-rerun-submit = Reejecutar
webhooks-rerun-no-payload = Este webhook aún no ha capturado una carga útil — actívalo una vez primero.
webhooks-rerun-no-payload-notice = Este webhook aún no se ha activado, así que no hay carga útil para reproducir. Actívalo una vez y luego vuelve para reejecutarlo con un prompt diferente.

# --- Historial de ejecuciones ---
webhooks-runs-link = ejecuciones
webhooks-runs-heading = Ejecuciones · { $name }
webhooks-runs-page-name = Ejecuciones del webhook
webhooks-runs-intro = Las activaciones y reejecuciones más recientes. Abre una ejecución para leer su conversación, o reejecuta su carga útil con un prompt diferente.
webhooks-runs-empty = Aún no hay ejecuciones. Activa el webhook para ver su historial aquí.
webhooks-run-open = abrir chat
webhooks-run-rerun = reejecutar
webhooks-run-source-fire = activado
webhooks-run-source-rerun = reejecución
webhooks-run-status-ok = ok
webhooks-run-status-error = error
webhooks-run-status-pending = en curso

# --- Reutilización de la conversación ---
webhooks-reuse-toggle-label = Reutilizar la conversación (cada activación continúa el chat anterior)
webhooks-reuse-rounds-prefix = reproduciendo las últimas
webhooks-reuse-rounds-suffix = rondas
webhooks-reuse-rounds-aria = Rondas de historial a reproducir

webhooks-toast-rerun-failed = Reejecución { $status }
webhooks-rotate-confirm = ¿Emitir un nuevo secreto de activación? La URL antigua deja de funcionar de inmediato.
webhooks-delete-confirm = ¿Eliminar este webhook?

# Los enlaces de la página de lista a lo que ha producido un webhook, y el
# editor en su propia ruta (`/webhooks/new`, `/webhooks/{id}/edit`).
webhooks-create-heading = Nuevo webhook
webhooks-new-page-title = Nuevo webhook
webhooks-edit-named-heading = Editar { $name }
webhooks-toast-created = Webhook creado.
webhooks-toast-saved = Webhook guardado.
webhooks-badge-reuses-chat = una sola conversación
webhooks-open-chat = Abrir chat
webhooks-open-chats = { $count ->
    [one] { $count } chat
   *[other] { $count } chats
}
webhooks-open-runs = { $count ->
    [one] { $count } ejecución
   *[other] { $count } ejecuciones
}
webhooks-reveal-done = Listo — volver a los webhooks
