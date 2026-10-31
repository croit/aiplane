# STATUS: llm-generated, unreviewed — pending native-speaker QA


scheduled-heading = Acciones programadas
scheduled-intro = Ejecuta un prompt automáticamente según una programación. Cada ejecución abre un nuevo chat que puedes leer aquí — elige un modelo, escribe el prompt y decide cuándo debe ejecutarse.
scheduled-create-submit = Crear acción programada
scheduled-list-heading = Tus acciones programadas
scheduled-list-empty = Aún no hay acciones programadas. Crea una con el botón de arriba.

scheduled-back = Atrás
scheduled-edit-heading = Editar acción programada
scheduled-save-submit = Guardar cambios

scheduled-name-label = Nombre
scheduled-name-placeholder = p. ej. Resumen diario de noticias
scheduled-model-label = Modelo
scheduled-model-placeholder = id del modelo (p. ej. gpt-4o-mini)
scheduled-prompt-label = Prompt
scheduled-prompt-placeholder = ¿Qué debe hacer el modelo en cada ejecución?
scheduled-tools-toggle-label = Permitir herramientas (búsqueda web, RAG, adjuntos) — igual que en el chat
scheduled-reuse-toggle-label = Continuar una conversación — cada ejecución escribe en el mismo chat en lugar de abrir uno nuevo
scheduled-reuse-rounds-prefix = enviar las últimas
scheduled-reuse-rounds-aria = Número de rondas de historial a repetir
scheduled-reuse-rounds-suffix = rondas

scheduled-builder-heading = Programación
scheduled-mode-hourly = Cada hora
scheduled-mode-daily = Diaria
scheduled-mode-weekly = Semanal
scheduled-mode-monthly = Mensual
scheduled-mode-advanced = Avanzada
scheduled-weekday-mon = Lun
scheduled-weekday-tue = Mar
scheduled-weekday-wed = Mié
scheduled-weekday-thu = Jue
scheduled-weekday-fri = Vie
scheduled-weekday-sat = Sáb
scheduled-weekday-sun = Dom
scheduled-on-day-label = El día
scheduled-of-every-month = de cada mes
scheduled-at-label = A las
scheduled-hour-aria = Hora
scheduled-minute-aria = Minuto
scheduled-of-every-hour = de cada hora
scheduled-timezone-label = Zona horaria
scheduled-cron-label = Expresión cron
scheduled-cron-help = Cinco campos: minuto hora día-del-mes mes día-de-la-semana.

scheduled-no-upcoming-runs = No hay próximas ejecuciones.
scheduled-next-runs-prefix = Próximas ejecuciones:{ " " }

scheduled-err-pick-weekday = Elige al menos un día de la semana.
scheduled-err-enter-cron = Introduce una expresión cron.


scheduled-toast-not-found = No existe esa acción programada.

scheduled-badge-active = activa
scheduled-badge-paused = pausada
scheduled-status-paused = Pausada
scheduled-next-run = Próxima ejecución: { $when }
scheduled-no-upcoming-run = Sin próxima ejecución
scheduled-last-success = Última: ✓ { $when }
scheduled-last-failure = Última: ✗ { $when }
scheduled-pause-title = Pausar
scheduled-resume-title = Reanudar
scheduled-edit-title = Editar
scheduled-delete-title = Eliminar
scheduled-delete-confirm = ¿Eliminar esta acción programada?
scheduled-preview-summary = { $summary } ({ $timezone })
scheduled-preview-next-runs = Próximas ejecuciones: { $runs }
scheduled-last-error = Último error: { $error }

# Los enlaces de la página de lista a lo que ha producido una programación, y
# el historial de ejecuciones detrás de ellos (`/scheduled/{id}/runs`).
scheduled-create-heading = Nueva acción programada
scheduled-new-page-title = Nueva acción programada
scheduled-edit-named-heading = Editar { $name }
scheduled-toast-created = Acción programada creada.
scheduled-toast-saved = Acción programada guardada.
scheduled-badge-reuses-chat = una sola conversación
scheduled-open-chat = Abrir chat
scheduled-open-chats = { $count ->
    [one] { $count } chat
   *[other] { $count } chats
}
scheduled-open-runs = { $count ->
    [one] { $count } ejecución
   *[other] { $count } ejecuciones
}
scheduled-never-run = Aún no se ha ejecutado
scheduled-runs-page-title = Historial de ejecuciones
scheduled-runs-heading = Ejecuciones · { $name }
scheduled-runs-intro = Cada ejecución registrada, la más reciente primero, con el chat que abrió.
scheduled-runs-empty = Aún no hay ejecuciones. Esta acción no se ha activado desde que se creó.
scheduled-run-open = abrir chat
scheduled-run-chat-deleted = chat eliminado
scheduled-run-status-ok = ok
scheduled-run-status-error = error
scheduled-run-status-pending = en curso

scheduled-organization-version = Versión publicada
scheduled-error-invalid-configuration = Revisa la versión publicada seleccionada, el modelo, el modo de conversación y la configuración de las herramientas y vuelve a guardar.
scheduled-error-configuration-conflict = Esta acción programada ha cambiado. Recarga su configuración actual antes de volver a guardarla.
scheduled-error-identity-retained = Este agente todavía tiene acciones programadas o webhooks. Detén las ejecuciones pendientes y elimina esas fuentes antes de eliminar el agente.
scheduled-organization-fresh = Cada ejecución inicia una nueva conversación privada. El modelo y el acceso a herramientas proceden de la versión publicada seleccionada.
scheduled-organization-model-missing = No se especifica ningún modelo en esta versión publicada.
scheduled-organization-version-required = Elige una versión publicada disponible antes de guardar.
