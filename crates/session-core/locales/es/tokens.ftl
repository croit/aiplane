# STATUS: llm-generated, unreviewed — pending native-speaker QA

tokens-page-heading = Tokens de API
tokens-intro = Tokens para aplicaciones y código que se conectan a AIplane. El valor completo solo se muestra al crear o renovar el token: cópialo y guárdalo en privado.

tokens-create-heading = Crear token
tokens-name-label = Nombre
tokens-name-placeholder = p. ej. laptop, ci-runner
tokens-ttl-label = TTL (días)
tokens-create-submit = Crear token

tokens-list-heading = Sus tokens
tokens-list-empty = Aún no hay tokens. Cree uno con el botón de arriba.

tokens-badge-revoked = revocado
tokens-badge-active = activo
tokens-remove-button = Eliminar
tokens-rotate-button = Rotar
tokens-rotate-title = Emitir un nuevo secreto para este token (conserva su nombre y configuración)
tokens-revoke-button = Revocar

tokens-row-meta = creado { $created } · último uso { $last_used } · expira { $expires }
tokens-last-used-never = nunca

tokens-tool-use-label = Uso de herramientas

tokens-mcp-allow-description = Las herramientas de conector que requieren aprobación no pueden solicitar confirmación a través de la API; al activarlo se ejecutan sin preguntar.

tokens-minted-heading = Token creado
tokens-minted-copy-warning = Copie el valor ahora — no podrá volver a verlo después.
tokens-copy-aria = Copiar token
tokens-minted-name = Nombre: { $name }

tokens-account-user-id-label = ID de usuario

# Web Push "turn complete" opt-in card (rendered by `render_push_card`; wired
# client-side by `ui/ts/push.ts`). Device-local notification settings.
tokens-push-enable = Activar en este dispositivo
tokens-push-disable = Desactivar en este dispositivo
tokens-push-on = Las notificaciones están activadas para este dispositivo.
tokens-push-enabled = Notificaciones activadas en este dispositivo.
tokens-push-disabled = Notificaciones desactivadas en este dispositivo.
tokens-push-error = No se pudo cambiar la configuración de notificaciones.

# Uso, lista de modelos permitidos y cuota por token (/tokens).
tokens-usage-line = este mes: { $requests } solicitudes · { $tokens } tokens · { $cost }
tokens-models-summary-restricted = Modelos: { $count } seleccionados
tokens-models-help = Desactivado, este token sigue tu propio acceso, incluidos los modelos añadidos más adelante. Activado, solo puede usar los modelos que marques: un modelo añadido después queda bloqueado hasta que también lo marques aquí.
tokens-models-restrict-label = Limitar este token a modelos concretos
tokens-models-saved-toast = Token limitado a { $count } modelos.
tokens-models-cleared-toast = El token puede usar todos tus modelos.
tokens-limits-add = Añadir cuota
tokens-limits-saved-toast = Cuota del token guardada.

# SPA-only: the Svelte /tokens row panels and their confirm prompts.
tokens-quota-max-placeholder = máx
tokens-revoke-confirm = ¿Revocar este token? Los clientes que lo usan dejan de funcionar de inmediato.
tokens-rotate-confirm = ¿Emitir un secreto nuevo? El anterior deja de funcionar de inmediato.
tokens-remove-confirm = ¿Eliminar definitivamente esta fila de token?

tokens-create-description = Genere un nuevo token Bearer para la API compatible con OpenAI.
tokens-tool-use-description = Permitir que este token llame a las herramientas del gateway (búsqueda web, RAG, …).
tokens-capabilities-summary = Capacidades
tokens-panel-close = Cerrar
tokens-edit-button = Editar
tokens-mcp-allow-label = Permitir herramientas MCP “ask” a través de la API
tokens-account-heading = Cuenta
tokens-signed-in-as = Conectado como { $email }
tokens-account-oidc-label = Roles OIDC
tokens-account-rbac-label = IDs de rol RBAC
tokens-roles-none = ninguno
tokens-roles-none-granted = ninguno concedido
tokens-push-heading = Notificaciones
tokens-push-description = Recibe una notificación en este dispositivo cuando termine una respuesta que iniciaste mientras estás fuera de la aplicación.
tokens-push-off = Las notificaciones están desactivadas para este dispositivo.
tokens-push-denied = Este navegador ha bloqueado las notificaciones. Permítelas en la configuración del navegador para activarlas.
tokens-push-unsupported = Este navegador no admite notificaciones.
tokens-models-none-picked = Marca al menos un modelo o desactiva el límite.
tokens-limits-help = Un tope solo para este token. Tu propio presupuesto sigue aplicándose, así que esto solo puede reducir lo que el token gasta, nunca ampliarlo.
tokens-limits-remove = Quitar
tokens-limits-admin-badge = fijada por el administrador
tokens-models-admin-set = Un operador también restringe este token a: { $models }. Tu selección solo puede reducir eso, no ampliarlo.

# Client setup guides and token-page tabs.
tokens-tab-tokens = Tokens
tokens-tab-guides = Guías de configuración
tokens-tab-account = Cuenta
tokens-guides-heading = Configurar un cliente
tokens-guides-intro = Elige una aplicación y sigue los pasos. Puedes usar un token en varias aplicaciones, pero un token por aplicación es más fácil de administrar.
tokens-guides-before = Primero crea un token en la pestaña Tokens y cópialo. El valor completo solo se muestra una vez. Guárdalo en privado.
tokens-guide-opencode = OpenCode
tokens-guide-claude = Claude Code
tokens-guide-python = Python (OpenAI)
tokens-opencode-step-1 = Crea un token en la pestaña Tokens y cópialo.
tokens-opencode-step-2 = En OpenCode, ejecuta /connect, elige Other, escribe aiplane como ID de proveedor y pega tu token.
tokens-opencode-step-3 = Guarda esta configuración en ~/.config/opencode/opencode.jsonc. Sustituye YOUR_MODEL_ID por el ID de un modelo de chat disponible.
tokens-opencode-finish = Ejecuta opencode y usa /models para elegir aiplane/YOUR_MODEL_ID.
tokens-claude-step-1 = Crea un token en la pestaña Tokens y cópialo.
tokens-claude-step-2 = Elige el ID de un modelo de chat disponible. Es posible que tu administrador haya creado un alias para Claude Code.
tokens-claude-step-3 = Pega estos comandos en tu terminal. Primero sustituye el token y el ID del modelo.
tokens-claude-finish = Claude Code usa la URL base sin /v1. Si aparece un error de modelo no encontrado, pregunta a tu administrador qué alias usar.
tokens-python-step-1 = Crea un token en la pestaña Tokens y cópialo.
tokens-python-step-2 = Instala el paquete Python de OpenAI con python -m pip install openai y define OPENAI_API_KEY con tu token en la terminal.
tokens-python-step-3 = Guarda este ejemplo como chat.py. Sustituye YOUR_MODEL_ID por el ID de un modelo de chat disponible.
tokens-python-finish = Ejecuta python chat.py. Guarda el token en una variable de entorno, no en el script.
tokens-guide-omp = Oh My Pi
tokens-guide-pi = Pi
tokens-omp-finish = Ejecuta omp y usa /model para elegir aiplane/YOUR_MODEL_ID.
tokens-omp-step-1 = Crea un token en la pestaña Tokens y cópialo.
tokens-omp-step-2 = Guarda esta configuración en ~/.omp/agent/models.yml. Sustituye antes el token y YOUR_MODEL_ID, y ajusta contextWindow al tamaño de contexto del modelo.
tokens-pi-finish = Ejecuta pi y usa /model para elegir aiplane/YOUR_MODEL_ID.
tokens-pi-step-1 = Crea un token en la pestaña Tokens y cópialo.
tokens-pi-step-2 = Guarda esta configuración en ~/.pi/agent/models.json. Sustituye antes el token y YOUR_MODEL_ID.
tokens-guides-model-note = ¿Necesitas un ID de modelo? Los modelos disponibles aparecen en el selector del chat o en GET /v1/models con tu token.

notifications-loading = Cargando los ajustes de notificaciones…
notifications-unavailable = Las notificaciones no están disponibles en esta pasarela. Pide a un administrador que revise la configuración de Push.
notifications-admin-settings-link = Abrir la configuración de Push
tokens-tile-models = Modelos
tokens-tile-tools = Herramientas
tokens-tile-budget = Presupuesto
tokens-save = Guardar
tokens-menu-aria = Acciones del token
tokens-expires-today = caduca hoy
tokens-expires-soon = { $days ->
    [one] caduca en { $days } día
   *[other] caduca en { $days } días
}
tokens-models-tile-all = Los { $count } modelos — sigue tu acceso
tokens-models-tile-some = { $count } de { $total } modelos
tokens-models-tile-compliant = Todos conformes con RGPD y cubiertos por NDA
tokens-models-tile-noncompliant = { $count ->
    [one] { $count } modelo sin cobertura RGPD o NDA
   *[other] { $count } modelos sin cobertura RGPD o NDA
}
tokens-models-tile-admin = Un operador permite { $count } de ellos
tokens-models-max-price = hasta { $price } por millón de tokens de salida
tokens-models-noncompliant-warning = { $count ->
    [one] { $count } modelo aquí no cumple el RGPD o no está cubierto por NDA. No envíes datos personales ni material confidencial con este token.
   *[other] { $count } modelos aquí no cumplen el RGPD o no están cubiertos por NDA. No envíes datos personales ni material confidencial con este token.
}
tokens-models-search = Buscar modelos
tokens-models-filter-gdpr = Conforme RGPD
tokens-models-filter-nda = Cubierto por NDA
tokens-models-filter-free = Sin coste
tokens-models-select-compliant = Seleccionar todos RGPD + NDA
tokens-models-select-none = Borrar selección
tokens-models-empty = Ningún modelo coincide.
tokens-models-alias = alias de { $target }
tokens-models-gdpr-ok = Conforme RGPD: los datos personales siguen protegidos.
tokens-models-nda-ok = Cubierto por un acuerdo de confidencialidad.
tokens-models-price-free = sin coste
tokens-models-price-tokens = { $input } entrada / { $output } salida por millón
tokens-models-price-per-images = { $price } por imagen
tokens-models-price-per-characters = { $price } por carácter
tokens-models-price-per-seconds = { $price } por segundo
tokens-models-kind-chat = Chat
tokens-models-kind-transcription = Voz a texto
tokens-models-kind-speech = Texto a voz
tokens-models-kind-embedding = Embedding
tokens-models-kind-rerank = Reordenación
tokens-models-kind-image = Imágenes
tokens-models-kind-system_one = System One
tokens-tools-tile-on = Permitidas
tokens-tools-tile-off = Desactivadas
tokens-tools-tile-pinned = { $count } siempre activas
tokens-tools-tile-mcp-allowed = Las herramientas MCP «Ask» se ejecutan sin preguntar
tokens-tools-tile-mcp-blocked = Herramientas MCP «Ask» bloqueadas
tokens-tools-capabilities-help = Qué herramientas y habilidades puede usar este token.
tokens-tools-saved-toast = Herramientas del token guardadas.
tokens-budget-tile-none = Sin límite propio
tokens-budget-tile-more = +{ $count } más
tokens-budget-owner-applies = Tu propio presupuesto también se aplica
tokens-budget-token-heading = Este token
tokens-budget-owner-heading = También se aplica: tu propio presupuesto
tokens-budget-owner-none = No tienes límite de presupuesto.
tokens-budget-dimension = Qué limitar
tokens-budget-window = Periodo
