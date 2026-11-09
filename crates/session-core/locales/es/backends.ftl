# STATUS: llm-generated, unreviewed — pending native-speaker QA


backends-status-down = caído
backends-status-up = activo

backends-inflight-label = en curso { $load }

# Editor CRUD de backends (añadir/editar/eliminar backends almacenados en la topología de la base de datos).
backends-apply-changes = Aplicar cambios
backends-field-name = Nombre
backends-field-base-url = URL base
backends-field-pool = Grupo
backends-field-pool-none = (ninguno)
backends-save-backend = Guardar backend
backends-delete-backend = Eliminar

backends-field-api-key = Clave API
backends-field-api-key-keep = dejar en blanco para conservar la clave actual
backends-field-api-key-stored = Clave almacenada: { $hint }

# An alias that is configured but would not route.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.
backends-status-drained = mantenimiento

# Filas de backend de la SPA: interruptor de drenaje, carga por hora y borrado.
backends-add-heading = Añadir backend
backends-name-taken = Ya existe un backend con este nombre: guardar lo sobrescribiría, incluidos su URL base, clave, modelos y pool. Elige otro nombre para añadir un segundo backend.
backends-field-api-key-placeholder = Clave API (almacenada cifrada)
backends-field-api-key-env = Variable de entorno de clave de API
backends-field-health-path = Ruta de estado
backends-field-pool-hint = Asigna este backend a un grupo. Un backend en varios grupos se reduce al elegido aquí.
backends-field-weight = Peso
backends-field-max-inflight = Máximo en curso
backends-field-models = Modelos (separados por comas; en blanco = descubrir vía /models)
backends-field-aliases = Alias (name=target por línea)
backends-field-extra-headers = Encabezados adicionales (nombre: valor por línea)
backends-field-extra-headers-hint = Se envía con cada petición a este backend, después de la clave API. Por ejemplo anthropic-version: 2023-06-01, que Anthropic exige.
backends-field-supports-edit = Admite edición de imágenes
backends-status-saturated = saturado
backends-auth-failed-title = El upstream rechazó las credenciales de la sonda de salud (401/403), así que el descubrimiento de modelos está desactivado y nada nuevo puede enrutarse por este backend. Revisa la clave API; si usa una variable de entorno, comprueba que esté definida.
backends-auth-failed = clave rechazada
backends-no-models-title = Este backend no anuncia ningún modelo, así que nada se enruta ahí y un alias simple no tiene a qué vincularse. Normalmente una sonda que nunca devolvió datos.
backends-no-models = no se anuncian modelos
backends-key-env-badge = clave: env { $var }
backends-key-env-unset-badge = env { $var } NO DEFINIDA
backends-enabled-hint = Desactiva para vaciar este backend por mantenimiento. Surte efecto de inmediato: no hace falta «Aplicar cambios». Sus modelos siguen siendo conocidos, así que los otros backends del pool asumen la carga y los clientes ven una interrupción temporal, nunca «modelo no encontrado».
backends-enabled-label = Acepta tráfico
backends-activity-summary = 15m { $m15 } · 30m { $m30 } · 60m { $m60 }
backends-aliases-label = alias:
backends-alias-target-title = alias → { $target }
backends-alias-disabled-title = alias simple desactivado — este backend sirve varios modelos; asígnele un destino explícito (formulario de mapeo)
backends-alias-disabled-label = { $name } (desactivado)
backends-fallback-offline-title = fallback_offline: se usa cuando todos los backends de un modelo conocido en este pool están caídos
backends-fallback-offline-badge = fuera de línea ↩ { $model }
backends-pool-empty = No hay backends en este pool.

backends-test-button = Probar conexión
backends-test-hint = Llama a esta URL con las credenciales de arriba. No se guarda nada.
backends-test-insert-hint = Ids de modelos anunciados: haz clic para completar la línea de alias donde está el cursor:
backends-test-ok = Alcanzable, autenticado ({ $source }), { $count } modelos descubiertos.
backends-test-ok-no-models = Alcanzable y autenticado ({ $source }), pero la respuesta no es una lista /models de OpenAI ni de TypeSafe, así que el descubrimiento no puede leerla. Este backend solo podrá servir los ids que indiques en «Modelos».
backends-test-auth-failed = Rechazado con HTTP { $status }: la credencial fue denegada ({ $source }). Hasta que se corrija, el descubrimiento de modelos queda desactivado y el backend no anuncia nada.
backends-test-http-error = { $url } respondió HTTP { $status }.
backends-test-upstream-said = Respuesta del upstream: { $detail }
backends-test-unreachable = No se pudo alcanzar { $url }: { $err }
backends-test-timeout = { $url } no respondió en { $secs }s.
backends-test-key-typed = con la clave escrita arriba
backends-test-key-stored = con la clave almacenada
backends-test-key-env = desde env { $var }
backends-test-key-env-unset = env { $var } NO ESTÁ DEFINIDA: la petición salió sin credencial
backends-test-key-none = sin credencial enviada
backends-error-base-url-required = la URL base es obligatoria

backends-parallel-mismatch = el servidor procesa { $parallel } a la vez
backends-detect-profile = identificado: { $profile }

# OpenRouter catalog: one backend, one key, the account's models, and a
# purchase-price ceiling per selected model.
openrouter-heading = OpenRouter
openrouter-description = One OpenRouter connection. Pick the models this gateway may use and the maximum you will pay for each; a model with no provider under its ceiling is not offered.
openrouter-connected = Connected
openrouter-key-label = Key
openrouter-credits = Credits
openrouter-refresh = Refresh prices
openrouter-backend = Backend
openrouter-pool = Pool
openrouter-search = Search models
openrouter-select-all = Select all
openrouter-deselect-all = Deselect all
openrouter-selected-count = { $count } selected
openrouter-model = Model
openrouter-price = List price
openrouter-context = Context
openrouter-ceiling-prompt = Max input $/1M
openrouter-ceiling-completion = Max output $/1M
openrouter-strategy = Provider strategy
openrouter-strategy-cheapest = Cheapest
openrouter-strategy-fastest = Fastest
openrouter-strategy-balanced = Balanced
openrouter-min-throughput = Min tokens/s
openrouter-available = Available
openrouter-over-ceiling = Over ceiling
openrouter-save = Save selection
openrouter-saved = Selection saved.
openrouter-no-models = No models match this filter.
openrouter-error = OpenRouter: { $message }
openrouter-select-model-first = Select at least one model.
openrouter-endpoints = Providers
openrouter-provider = Provider
openrouter-provider-field = Provider
openrouter-provider-openai = OpenAI-compatible
openrouter-provider-openrouter = OpenRouter
openrouter-provider-hint = OpenRouter reads the account catalog and enforces the per-model price ceiling on every request. Other providers are plain OpenAI-compatible HTTP.
openrouter-ceiling-required = Every selected model needs both price ceilings.
openrouter-pool-empty = This backend is not in a pool yet. Add it to a chat, embedding, speech, transcription, image, rerank or System One pool first.
openrouter-mixed-pool = This pool also contains { $backends }. A pool has one model list, so saving here pins theirs too; put OpenRouter in its own pool.
openrouter-max-price = Max $/1M
openrouter-voices = Voces
