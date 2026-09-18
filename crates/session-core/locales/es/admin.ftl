# Strings owned by `gateway/src/rama_server/pages/admin.rs` — la página
# `/admin/models`.

admin-heading = Modelos

admin-col-model = Modelo

admin-not-configured = sin configurar

admin-badge-ctx = CTX

admin-save-model = Guardar modelo
admin-edit-model = Editar
admin-edit-model-page-title = Editar modelo
admin-model-not-found = No existe ese modelo. Puede que ningún backend lo ofrezca ya.
admin-clear-overrides = Borrar todos los ajustes
admin-cancel = Cancelar

admin-toml-defaults-label = Valores de muestreo (TOML)

# Precios por modelo para la contabilidad de costes (precio por 1 M de tokens, entrada / salida).
admin-price-in-label = Precio ent
admin-price-out-label = Precio sal
admin-price-in-placeholder = sin precio
admin-price-out-placeholder = sin precio

# Ventana de contexto (controla la compactación automática).
admin-context-window-full-label = Ventana de contexto (tokens)
admin-context-window-placeholder = predet.

# Modelos predeterminados por función.
admin-defaults-heading = Modelos predeterminados

# Capacidades del modelo (tri-estado) + modelos de reserva.
admin-cap-tools = Herramientas

# Backend de búsqueda web (herramienta `search_web`).
admin-search-heading = Búsqueda web
admin-search-provider-label = Proveedor
admin-search-provider-searxng = SearXNG (autoalojado)
admin-search-provider-brave = Brave Search API
admin-search-searxng-url-label = URL base de SearXNG
admin-search-searxng-url-placeholder = https://searxng.example.com
admin-search-brave-key-label = Clave de API de Brave
admin-search-brave-key-placeholder = déjalo vacío para conservar la clave actual
admin-search-save = Guardar búsqueda web
admin-search-saved = ajustes de búsqueda web guardados

# ─── SPA de administración SvelteKit ─────────────────────────────────────────
# La comprobación de rol del shell de administración, el catálogo de flujos de
# trabajo de /admin/comfyui y las partes del editor de modelos que la página
# antigua no tenía.

admin-needs-admin-role = Estas páginas requieren el rol de administrador.
admin-overwrite-existing = sobrescribir el existente
admin-comfyui-reload = Recargar catálogo
admin-comfyui-reloaded = Catálogo recargado — { $count } flujo(s) cargado(s).
admin-comfyui-empty = No hay flujos de trabajo cargados — revisa el directorio de contenido.
admin-comfyui-heading = Catálogo de flujos ComfyUI
admin-comfyui-page-title = ComfyUI — Catálogo de flujos de trabajo
admin-comfyui-intro = Worker ComfyUI sin interfaz. La puerta de enlace expone cada flujo cargado como una herramienta comfyui_<id> que puede invocar el modelo. Los usuarios nunca ven ComfyUI.
admin-comfyui-not-configured = Sin configurar
admin-comfyui-not-configured-help = Activa ComfyUI en Configuración, establece su URL base y el directorio de flujos, y reinicia la puerta de enlace.
admin-comfyui-operator-config = Configuración del operador
admin-comfyui-worker-url = URL base del worker
admin-comfyui-content-directory = Directorio de contenido
admin-comfyui-timeout = Tiempo límite del flujo
admin-comfyui-poll-interval = Intervalo de consulta de la cola
admin-comfyui-config-help = El operador administra el directorio de contenido y este no forma parte del repositorio público. Edita allí los manifiestos y pulsa Recargar — no hace falta reiniciar.
admin-comfyui-loaded-workflows = Flujos cargados
admin-comfyui-node = nodo { $id }
admin-comfyui-parameters = Parámetros
admin-comfyui-required = obligatorio
admin-comfyui-recent-jobs = Trabajos recientes
admin-comfyui-reloaded-skipped = Catálogo recargado — { $count } flujo(s) cargado(s), { $skipped } omitido(s).
admin-comfyui-max-concurrent = Trabajos simultáneos
admin-comfyui-tab-workflows = Flujos de trabajo
admin-comfyui-tab-jobs = Ejecuciones
admin-comfyui-jobs-page-title = ComfyUI — Ejecuciones
admin-comfyui-jobs-intro = Todos los flujos que los modelos han ejecutado recientemente, del más nuevo al más antiguo: qué produjo, cuánto tardó y la conversación que lo pidió.
admin-comfyui-jobs-window = Las { $count } ejecuciones más recientes registradas por la pasarela.
admin-comfyui-jobs-empty = Aún no hay ejecuciones registradas.
admin-comfyui-worker-status = Worker
admin-comfyui-worker-reachable = Accesible
admin-comfyui-worker-unreachable = Inaccesible
admin-comfyui-worker-checking = Comprobando…
admin-comfyui-worker-queue = { $running } en ejecución · { $pending } en cola
admin-comfyui-worker-software = ComfyUI { $version } · Python { $python } · PyTorch { $torch }
admin-comfyui-worker-vram = { $free } libres de { $total }
admin-comfyui-search-placeholder = Buscar flujos y parámetros
admin-comfyui-search-empty = Ningún flujo coincide con esa búsqueda.
admin-comfyui-filename-prefix = Prefijo de salida
admin-comfyui-required-count = { $required } de { $total } obligatorios
admin-comfyui-no-params = Este flujo no admite parámetros.
admin-comfyui-detail-empty = Elige un flujo para ver el contrato que recibe el modelo.
admin-comfyui-param-column = Parámetro
admin-comfyui-param-description-column = Descripción
admin-comfyui-filter-all = Todas
admin-comfyui-filter-completed = Completadas
admin-comfyui-filter-pending = Pendientes
admin-comfyui-filter-failed = Fallidas
admin-comfyui-stats-heading = Fiabilidad por flujo
admin-comfyui-col-workflow = Flujo
admin-comfyui-col-runs = Ejecuciones
admin-comfyui-col-failed = Fallidas
admin-comfyui-col-median = Mediana
admin-comfyui-col-status = Estado
admin-comfyui-col-duration = Duración
admin-comfyui-col-when = Inicio
admin-comfyui-col-result = Resultado
admin-comfyui-job-open = Abrir conversación
admin-comfyui-job-still-running = aún en curso
admin-comfyui-refresh = Actualizar
admin-clear-overrides-confirm = ¿Descartar todas las anulaciones guardadas de { $model }?
admin-cap-no-fallback = (ninguno)

admin-page-title = Modelos — AIplane

admin-intro-prefix = Ajustes por modelo — precios, ventana de contexto, razonamiento, capacidades y valores de muestreo — aplicados a

admin-intro-every = cada

admin-intro-middle = solicitud de este modelo, de cualquier usuario o token, salvo que quien llame defina el mismo valor, el cual

admin-intro-always-wins = siempre prevalece

admin-intro-suffix = . Los modelos de chat, los alias y otras clases están todos en una lista.

admin-no-models = Aún no se anuncian modelos. En cuanto un backend upstream esté accesible, aparecerá aquí.

admin-filter-placeholder = Filtrar modelos…

admin-filter-all = Todos

admin-filter-chat = chat

admin-filter-other = otras clases

admin-filter-aliases = alias

admin-filter-configured = solo configurados

admin-col-kind = Clase

admin-col-price = Precio ent/sal

admin-col-context = Contexto

admin-col-reasoning = Razonamiento

admin-col-configured = Configurado

admin-value-default = predeterminado

admin-value-na = n/d

admin-alias-inherits = hereda los ajustes del destino

admin-reasoning-auto-resolved = Auto → { $style }

admin-badge-price = PRECIO

admin-badge-budget = PRESUP

admin-badge-caps = CAPS

admin-badge-toml = TOML

admin-other-price-note = El muestreo, el razonamiento y el contexto no se aplican a esta clase — solo los precios, para la contabilidad de costes.

admin-toml-placeholder-header = # Claves comunes (vLLM/OpenAI):

admin-reasoning-style-label = Estilo de razonamiento

admin-reasoning-style-aria = Estilo de razonamiento

admin-reasoning-auto = Automático

admin-reasoning-none = ninguno

admin-reasoning-qwen = Qwen (vLLM)

admin-reasoning-openai = OpenAI

admin-reasoning-glm = GLM / z.AI

admin-reasoning-anthropic = Anthropic
admin-reasoning-ollama = Ollama

admin-effort-standard = Estándar

admin-effort-deep = Profundo

admin-effort-max = Máx

admin-budget-placeholder = predeterminado

admin-budget-hint = Tokens de pensamiento máximos por nivel de esfuerzo. Vacío = valor predeterminado del backend (sin límite). «Fast» desactiva el razonamiento.

admin-effort-default-option = (predeterminado)

admin-effort-hint = Esfuerzo de razonamiento por nivel. Vacío = valor predeterminado integrado. «Fast» desactiva el razonamiento.

admin-saved-model = `{ $model }` guardado — efectivo de inmediato

admin-cleared-defaults = ajustes borrados para `{ $model }`

admin-price-label = { $cur }/{ $unit }

admin-price-unit-tokens = 1 M de tokens

admin-price-unit-images = imagen

admin-price-unit-characters = carácter

admin-price-unit-seconds = segundo

admin-alias-chip = alias

admin-defaults-intro = Elige el modelo preseleccionado para cada función. Vacío = el primer modelo disponible (comportamiento anterior).

admin-defaults-chat-label = Chat

admin-defaults-voice-label = Voz (transcripción)

admin-defaults-image-label = Generación de imágenes

admin-defaults-embedding-label = Embedding (RAG)

admin-defaults-first-option = Primero disponible

admin-defaults-saved = modelo predeterminado establecido en `{ $model }`

admin-defaults-cleared = modelo predeterminado restablecido

admin-capabilities-heading = Capacidades

admin-cap-vision = Visión

admin-cap-structured-output = Salida estructurada

admin-cap-audio-input = Entrada de audio

admin-cap-pdf-input = Entrada de PDF

admin-cap-parallel-tools = Herramientas en paralelo

admin-cap-unknown = Desconocido

admin-cap-enabled = Activado

admin-cap-disabled = Desactivado

admin-cap-fallback-vision = Reserva para visión

admin-cap-fallback-tools = Reserva para herramientas

admin-search-intro = Qué backend responde a la herramienta `search_web` del asistente. SearXNG solo necesita una URL base y no cuesta nada por consulta si ejecutas tu propia instancia; Brave necesita una clave de API. La clave se cifra en reposo.

admin-search-brave-key-set = Hay una clave almacenada (cifrada).

admin-search-brave-key-unset = No hay clave almacenada.

admin-search-brave-key-clear = Eliminar la clave almacenada

# Procedencia de la ventana de contexto.
admin-context-detected = Detectado: { $window } tokens
admin-context-unreported = Este backend no informa de una ventana de contexto para este modelo. Indíquela aquí o compruébela en el servidor.
admin-context-exceeds-detected = Este backend informa de { $window } tokens. Los valores superiores no se compactan: el servidor los trunca en silencio.
auto-route-heading = Rutas automáticas de modelos
auto-route-description = Expone un alias de modelo estándar que elige el mejor modelo permitido para cada solicitud.
auto-route-add = Añadir ruta automática
auto-route-privacy = El selector recibe el contenido necesario para clasificar, incluso si el modelo de generación elegido es local.
auto-route-empty = No hay rutas automáticas configuradas.
auto-route-needs-selector = Añada un upstream System One antes de crear una ruta automática.
auto-route-needs-candidates = Añada al menos dos modelos de chat distintos antes de crear una ruta automática.
auto-route-open-upstreams = Abrir upstreams
auto-route-candidates = Candidatos
auto-route-edit = Editar
auto-route-delete = Eliminar
auto-route-delete-confirm = ¿Eliminar la ruta automática «{ $alias }»?
auto-route-deleted = Se eliminó la ruta automática «{ $alias }».
auto-route-saved = Se guardó la ruta automática «{ $alias }».
auto-route-alias = Alias del modelo
auto-route-alias-help = Los clientes usan este valor en el campo de modelo estándar.
auto-route-selector = Modelo selector
auto-route-selector-help = Un modelo de un upstream System One que elige una clave de candidato.
auto-route-editor-help = Empiece en modo sombra, revise las decisiones de enrutamiento y active la ruta cuando su política esté lista.
auto-route-objective = Objetivo de optimización
auto-route-objective-quality = Calidad
auto-route-objective-balanced = Equilibrado
auto-route-objective-cost = Coste
auto-route-rollout = Modo de despliegue
auto-route-rollout-shadow = Sombra (solo medir)
auto-route-rollout-active = Activo
auto-route-rollout-help = El modo sombra registra decisiones, pero envía el tráfico al destino alternativo. El modo activo aplica la decisión del selector.
auto-route-confidence = Confianza mínima
auto-route-confidence-help = Las decisiones con menor confianza usan el destino alternativo.
auto-route-timeout = Tiempo límite del selector (ms)
auto-route-instructions = Instrucciones de enrutamiento
auto-route-candidate-add = Añadir candidato
auto-route-candidates-help = Describa cada modelo según las solicitudes que debe atender. El selector ve la clave y la descripción, no el nombre del modelo objetivo.
auto-route-candidate-key = Clave opaca
auto-route-candidate-target = Modelo objetivo o alias estático
auto-route-candidate-description = Cuándo debe usarse este candidato
auto-route-fallback = Objetivo alternativo
auto-route-session-affinity = Mantener la sesión en el primer modelo elegido
auto-route-session-affinity-help = Reutiliza el destino efectivo para el mismo cliente autenticado y la misma sesión hasta que venza el TTL.
auto-route-session-ttl = TTL de afinidad de sesión (segundos)
auto-route-cancel = Cancelar
auto-route-save = Guardar ruta
auto-route-saving = Guardando…
auto-route-decisions = Decisiones de enrutamiento recientes
auto-route-result = Objetivo efectivo
auto-route-latency = Latencia del selector
auto-route-reason-selected = Seleccionado
auto-route-reason-shadow = Sugerencia en sombra
auto-route-reason-low-confidence = Confianza baja
auto-route-reason-selector-error = Error del selector
auto-route-reason-session-affinity = Afinidad de sesión
auto-route-error-unavailable = Ningún destino configurado puede atender esta solicitud ahora. Compruebe la disponibilidad y las capacidades de los candidatos e inténtelo de nuevo.
auto-route-error-internal = No se pudo cargar la ruta automática. Revise los registros de la puerta de enlace e inténtelo de nuevo.
auto-route-error-invalid-body = La solicitud de ruta automática tiene un formato incorrecto. Revise los campos enviados e inténtelo de nuevo.
auto-route-error-invalid-config = La configuración de la ruta automática no es válida. Revise los candidatos, la alternativa, la confianza y los tiempos de espera.
auto-route-error-nested = Las rutas automáticas no pueden apuntar a otras rutas automáticas. Elija alias estáticos o identificadores de modelo.
auto-route-error-missing-alias = Falta el alias de la ruta automática en la URL de la solicitud.
auto-route-error-not-found = La ruta automática no existe. Actualice la lista de rutas e inténtelo de nuevo.
