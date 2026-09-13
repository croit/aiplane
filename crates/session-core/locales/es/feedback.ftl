# El widget de comentarios: el botón flotante, el diálogo (formulario,
# entrada por voz, anotación de capturas, adjuntos, consentimiento de
# diagnósticos), la confirmación del gestor público y los errores de
# `/api/v0/feedback*`.

feedback-fab-aria = Enviar comentarios
feedback-fab-title = Enviar comentarios

feedback-dialog-heading = Enviar comentarios
feedback-dialog-blurb = La página, tu navegador y la actividad reciente de consola y red se adjuntan automáticamente.
feedback-close-aria = Cerrar
feedback-cancel-button = Cancelar
feedback-submit-button = Enviar comentarios
feedback-sending = Enviando…

feedback-title-label = Título
feedback-title-placeholder = Resumen breve
feedback-description-label = Descripción
feedback-description-placeholder = ¿Qué ha pasado o qué te gustaría?
feedback-business-label = Valor de negocio
feedback-business-placeholder = ¿Por qué importa? ¿A quién afecta?
feedback-acceptance-label = Criterios de aceptación
feedback-acceptance-placeholder = ¿Cuándo está terminado?
feedback-priority-label = Prioridad
feedback-priority-low = Baja
feedback-priority-medium = Media
feedback-priority-high = Alta

# Entrada por voz: una grabación rellena todos los campos de arriba.
feedback-voice-button-label = Rellenar por voz
feedback-voice-button-title = Toca, describe el problema y vuelve a tocar: rellenaremos los campos de abajo
feedback-voice-stop-label = Parar y rellenar
feedback-voice-working-label = Transcribiendo…
feedback-voice-applied = Rellenado a partir de tu grabación: revísalo y envíalo.
feedback-voice-no-speech = No se ha detectado voz: inténtalo de nuevo.

# Captura de pantalla y anotación.
feedback-shot-label = Captura de pantalla
feedback-shot-status-capturing = Capturando…
feedback-shot-status-attached = Adjuntada: dibuja encima para anotar
feedback-shot-status-none = Sin captura de pantalla
feedback-shot-status-failed = Captura de pantalla no disponible
feedback-shot-capture = Añadir una captura
feedback-shot-recapture = Volver a capturar
feedback-shot-remove = Quitar
feedback-shot-exact = Píxel exacto
feedback-shot-exact-title = Captura los píxeles reales en pantalla mediante el selector de compartición de pantalla del navegador: incluye canvas, WebGL y marcos incrustados que la captura normal no puede reproducir.
feedback-shot-exact-cancelled = Compartición de pantalla cancelada: se conserva la captura actual.
feedback-shot-capture-failed = No se ha podido hacer la captura de pantalla.

feedback-shot-annotate = Anotar
feedback-annotate-heading = Anotar la captura de pantalla
feedback-annotate-done = Volver al formulario
feedback-annotate-hint = Arrastra para dibujar. Usa la herramienta de mover (o el botón central del ratón) para desplazarte por una vista ampliada; ctrl/⌘ + rueda para hacer zoom.
feedback-tool-pan-title = Mover
feedback-zoom-preset-title = Haz clic para ajustar toda la captura; vuelve a hacer clic para ocupar todo el ancho
feedback-zoom-fit-label = Ajustar
feedback-zoom-width-label = Ancho

feedback-tool-rect-title = Rectángulo
feedback-tool-arrow-title = Flecha
feedback-tool-pen-title = Mano alzada
feedback-tool-text-title = Texto
feedback-tool-redact-title = Ocultar / censurar (bloque relleno)
feedback-tool-text-prompt = Texto de la anotación
feedback-color-aria = Color
feedback-undo-title = Deshacer
feedback-redo-title = Rehacer
feedback-clear-annot-title = Borrar anotaciones
feedback-clear-annot-label = Borrar
feedback-zoom-out-title = Alejar
feedback-zoom-in-title = Acercar

# Imágenes adicionales, pegadas o soltadas sobre el formulario.
feedback-attachments-label = Imágenes
feedback-attachments-count = { $count } de { $max }
feedback-attachments-hint = Pega o suelta imágenes aquí para adjuntarlas.
feedback-attachments-drop = Suelta para adjuntar
feedback-attachments-remove = Quitar la imagen
feedback-attachments-too-many = Como máximo { $max } imágenes.
feedback-attachments-too-large = Esa imagen supera los { $max } MB.
feedback-attachments-invalid = Solo se pueden adjuntar archivos de imagen.

# Consentimiento de diagnósticos. Ambos vienen activados; el registro de
# conversación solo aparece en una página de conversación.
feedback-log-browser-label = Enviar el registro de actividad del navegador (consola + red)
feedback-log-chat-label = Enviar el registro de conversación y herramientas
feedback-diagnostics-label = Mostrar los datos que se adjuntarán

feedback-confirm-heading = ¿Seguro?
feedback-confirm-public-p1-prefix = Estos comentarios abren una incidencia en nuestro gestor
feedback-confirm-public-p1-strong = público
feedback-confirm-public-p1-suffix = . Cualquiera puede leerla.
feedback-confirm-private-p2-prefix = Asegúrate de que tu captura y los datos enviados no contienen
feedback-confirm-private-p2-strong = ninguna información personal o privada
feedback-confirm-private-p2-suffix = (nombres, correos, tokens, datos de clientes, …).
feedback-confirm-cancel-button = No, quiero editarlo
feedback-confirm-ok-button = Sí, enviar

feedback-thanks-heading = Gracias
feedback-thanks-body = Tus comentarios se han registrado como incidencia.
feedback-thanks-issue = Registrado como incidencia n.º { $number }.
feedback-thanks-open = Abrir la incidencia
feedback-done-button = Hecho

feedback-err-no-session = No hay sesión activa
feedback-err-session-lookup-failed = Fallo al buscar la sesión
feedback-err-body-read = { $error }
feedback-err-empty-transcript = Transcripción vacía
feedback-err-malformed-json = JSON mal formado: { $error }
feedback-err-no-chat-model = No hay ningún modelo de chat disponible para la extracción
feedback-err-extraction-failed = Fallo en la extracción: { $error }
feedback-err-not-configured = Los comentarios no están configurados
feedback-err-title-required = El título es obligatorio (al menos 4 caracteres)
feedback-err-description-required = La descripción es obligatoria
feedback-err-submit-failed = No se ha podido crear la incidencia: inténtalo de nuevo
feedback-err-network = Error de red: { $error }
