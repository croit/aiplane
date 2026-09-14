# STATUS: llm-generated, unreviewed — pending native-speaker QA

tokens-page-heading = Tokens de API
tokens-intro = Tokens Bearer para la API compatible con OpenAI. El texto plano solo se muestra al crearlo — guárdelo en un lugar seguro.

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

tokens-tool-use-aria = Uso de herramientas
tokens-tool-use-label = Uso de herramientas

tokens-mcp-allow-description = Las herramientas de conector que requieren aprobación no pueden solicitar confirmación a través de la API; al activarlo se ejecutan sin preguntar.

tokens-minted-heading = Token creado
tokens-minted-copy-warning = Copie el valor ahora — no podrá volver a verlo después.
tokens-copy-aria = Copiar token
tokens-minted-name = Nombre: { $name }

tokens-account-user-id-label = ID de usuario

tokens-mcp-ask-enabled-toast = Herramientas MCP "ask" a través de la API activadas para este token.
tokens-mcp-ask-disabled-toast = Herramientas MCP "ask" a través de la API desactivadas para este token.

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
tokens-models-summary-all = Modelos: todos
tokens-models-summary-restricted = Modelos: { $count } seleccionados
tokens-models-help = Desactivado, este token sigue tu propio acceso, incluidos los modelos añadidos más adelante. Activado, solo puede usar los modelos que marques: un modelo añadido después queda bloqueado hasta que también lo marques aquí.
tokens-models-restrict-label = Limitar este token a modelos concretos
tokens-models-save = Guardar modelos
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
tokens-mcp-allow-aria = Permitir herramientas MCP en modo "ask" a través de la API
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
tokens-limits-summary-none = Cuota: ninguna
tokens-limits-summary-some = Cuota: { $count } regla(s)
tokens-limits-help = Un tope solo para este token. Tu propio presupuesto sigue aplicándose, así que esto solo puede reducir lo que el token gasta, nunca ampliarlo.
tokens-limits-remove = Quitar
tokens-limits-removed-toast = Cuota del token eliminada.
tokens-limits-admin-badge = fijada por el administrador
tokens-models-admin-set = Un operador también restringe este token a: { $models }. Tu selección solo puede reducir eso, no ampliarlo.
