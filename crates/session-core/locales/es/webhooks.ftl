# STATUS: llm-generated, unreviewed — pending native-speaker QA

webhooks-heading = Webhooks
webhooks-intro = Ejecuta un prompt cuando un servicio externo llame a una URL. Obtienes una URL de activación secreta; lo que el llamante envíe en el cuerpo de la solicitud se añade a tu prompt, y la ejecución se abre como un nuevo chat que puedes leer aquí.
webhooks-edit-heading = Editar webhook
webhooks-list-empty = Aún no hay webhooks. Crea uno arriba.

webhooks-name-label = Nombre
webhooks-name-placeholder = p. ej. Resumen de despliegue
webhooks-model-label = Modelo
webhooks-model-placeholder = ID del modelo
webhooks-prompt-placeholder = ¿Qué debe hacer el modelo con los datos entrantes?

webhooks-reveal-heading = Tu URL de activación
webhooks-reveal-note = Cópiala ahora — solo se muestra una vez. Cualquiera con esta URL puede activar el webhook. ¿La perdiste? Rótala para obtener una nueva.
webhooks-copy = Copiar

webhooks-badge-active = Activo
webhooks-badge-paused = En pausa
webhooks-mode-sync = Espera la respuesta

webhooks-pause-title = Pausar
webhooks-resume-title = Reanudar
webhooks-rotate-title = Rotar secreto
webhooks-edit-title = Editar
webhooks-delete-title = Eliminar

# --- Reejecutar con un prompt diferente ---
webhooks-toast-rerun-started = Reejecución completada — abriendo la conversación…

# --- Historial de ejecuciones ---
webhooks-runs-empty = Aún no hay ejecuciones. Activa el webhook para ver su historial aquí.
webhooks-run-open = abrir chat
webhooks-run-rerun = reejecutar

# SPA-only: the Svelte /webhooks page — inline form, run list, rerun composer.
webhooks-new-heading = Nuevo webhook
webhooks-prompt-untrusted-label = Prompt (la carga útil llega como entrada no confiable)
webhooks-runs-show = Ejecuciones
webhooks-runs-hide = Ocultar ejecuciones
webhooks-rerun-prompt-label = Prompt de reejecución — la carga útil guardada se reproduce a través de este
webhooks-rerun-latest = Reejecutar la última carga útil
webhooks-rerun-running = Ejecutando…
webhooks-toast-rerun-failed = Reejecución { $status }
webhooks-rotate-confirm = ¿Emitir un nuevo secreto de activación? La URL antigua deja de funcionar de inmediato.
webhooks-delete-confirm = ¿Eliminar este webhook?
