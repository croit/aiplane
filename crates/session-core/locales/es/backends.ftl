# STATUS: llm-generated, unreviewed — pending native-speaker QA
# The upstream board and the backend editor: the pool band's status word, the
# editor's fields, and the connection-test outcomes.
backends-status-up = activo


# Editor CRUD de backends (añadir/editar/eliminar backends almacenados en la topología de la base de datos).
backends-field-name = Nombre
backends-field-base-url = URL base
backends-field-pool = Grupo
backends-field-pool-none = (ninguno)
backends-field-pools = Pools
backends-field-pools-hint = Un pool por cada tipo de modelo que sirve la cuenta. Un tipo se une a un pool vacío existente de ese tipo, recibe un pool creado en el mismo guardado, o no se sirve. No se ofrecen pools que ya contengan otro backend — mezclarlos fijaría ese backend a esta lista.
backends-pool-not-served = No se sirve
backends-pool-new = Nueva: openrouter-{ $kind }
backends-save-backend = Guardar backend
backends-delete-backend = Eliminar
backends-delete-confirm = ¿Eliminar el backend «{ $name }»? Su pool conserva los demás backends.
backends-advanced = Ajustes avanzados

backends-field-api-key = Clave API
backends-field-api-key-keep = dejar en blanco para conservar la clave actual
backends-field-api-key-stored = Clave almacenada: { $hint }
backends-field-api-key-clear = Eliminar la clave guardada

# An alias that is configured but would not route.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.

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
backends-aliases-managed = Los alias se editan en la pestaña Alias.
backends-field-extra-headers = Encabezados adicionales (nombre: valor por línea)
backends-field-extra-headers-hint = Se envía con cada petición a este backend, después de la clave API. Por ejemplo anthropic-version: 2023-06-01, que Anthropic exige.
backends-field-supports-edit = Admite edición de imágenes
backends-no-models-title = Este backend no anuncia ningún modelo, así que nada se enruta ahí y un alias simple no tiene a qué vincularse. Normalmente una sonda que nunca devolvió datos.
backends-no-models = sin modelos
backends-key-env-badge = clave: env { $var }
backends-key-env-unset-badge = env { $var } NO DEFINIDA
backends-enabled-hint = Desactiva para vaciar este backend por mantenimiento. Surte efecto de inmediato. Sus modelos siguen siendo conocidos, así que los otros backends del pool asumen la carga y los clientes ven una interrupción temporal, nunca «modelo no encontrado».
backends-enabled-label = Recibe tráfico ({ $name })
backends-activity-summary = 15m { $m15 } · 30m { $m30 } · 60m { $m60 }
backends-alias-target-title = alias → { $target }
backends-alias-missing-title = alias → { $target } — este backend no anuncia ese modelo
backends-alias-disabled-title = alias simple desactivado — este backend sirve varios modelos; asígnele un destino explícito (formulario de mapeo)
backends-alias-disabled-label = { $name } (desactivado)
backends-fallback-offline-title = fallback_offline: se usa cuando todos los backends de un modelo conocido en este pool están caídos
backends-fallback-offline-badge = fuera de línea ↩ { $model }
backends-pool-empty = No hay backends en este pool.

backends-test-button = Probar conexión
backends-test-hint = Llama a esta URL con las credenciales de arriba. No se guarda nada.
backends-test-models = Ids de modelo anunciados:
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
openrouter-description = Una conexión OpenRouter. Elige los modelos que esta pasarela puede usar y, si quieres un límite, el máximo que pagarás por cada uno; un modelo sin proveedor bajo su límite no se ofrece.
openrouter-connected = Conectado
openrouter-key-label = Clave
openrouter-credits = Crédito
openrouter-refresh = Actualizar precios
openrouter-backend = Backend
openrouter-search = Buscar modelos
openrouter-select-all = Seleccionar todo
openrouter-deselect-all = Deseleccionar todo
openrouter-selected-count = { $count } seleccionados
openrouter-model = Modelo
openrouter-models = Modelos
openrouter-filter-all = Todos
openrouter-filter-selected = Seleccionados
openrouter-filter-fitting = Compatibles
openrouter-price = Precio de lista
openrouter-context = Contexto
openrouter-ceiling-prompt = Máx. entrada $/1M
openrouter-in = Entrada
openrouter-out = Salida
openrouter-max-input = Máx. entrada
openrouter-max-output = Máx. salida
openrouter-provider-input-second = Entrada $/s
openrouter-provider-output-second = Salida $/s
openrouter-ceiling-prompt-second = Máx. entrada $/s
openrouter-ceiling-completion-second = Máx. salida $/s
openrouter-max-price-second = Máx. $/s
openrouter-cheapest-second = Más barato: { $provider } a { $prompt } / { $completion } por segundo de audio
openrouter-per-million = /1M
openrouter-per-second = /s
openrouter-show-models = Mostrar modelos
openrouter-hide-models = Ocultar modelos
openrouter-ceiling-invalid = Los límites deben estar vacíos o ser un número no negativo.
openrouter-ceiling-completion = Máx. salida $/1M
openrouter-strategy = Estrategia de proveedor
openrouter-strategy-cheapest = Más barata
openrouter-strategy-fastest = Más rápida
openrouter-strategy-balanced = Equilibrada
openrouter-min-throughput = Mín. tokens/s
openrouter-over-ceiling = Por encima del límite
openrouter-save = Guardar selección
openrouter-saved = Selección guardada.
openrouter-no-models = Ningún modelo coincide con este filtro.
openrouter-error = OpenRouter: { $message }
openrouter-select-model-first = Selecciona al menos un modelo.
openrouter-endpoints = Proveedores
openrouter-provider-input = Entrada $/1M
openrouter-provider-output = Salida $/1M
openrouter-provider-cache-read = Caché lectura
openrouter-provider-throughput = tok/s
openrouter-provider-latency = Latencia
openrouter-provider-uptime = Disponibilidad
openrouter-provider-limit = Tu límite
openrouter-usage = Consumo
openrouter-aria-input-ceiling = { $model } límite de entrada
openrouter-aria-output-ceiling = { $model } límite de salida
openrouter-aria-strategy = { $model } estrategia de proveedor
openrouter-aria-min-throughput = { $model } rendimiento mínimo
openrouter-cheapest-provider = Más barato
openrouter-provider-fits = incluido
openrouter-provider-excluded = excluido
openrouter-providers-fit = { $fit } de { $total } proveedores disponibles
openrouter-providers-hidden = { $count ->
    [one] { $count } proveedor oculto por tus límites
   *[other] { $count } proveedores ocultos por tus límites
  }
openrouter-providers-none-fit = Ningún proveedor cumple tus límites — sube o vacía Máx. entrada / Máx. salida.
openrouter-providers-fit-short = { $fit } de { $total } incluidos
openrouter-price-unknown = precio sin comprobar
openrouter-price-unknown-hint = Los precios de los proveedores de este modelo aún no se han leído — abre la fila para verlos.
openrouter-cheapest = Más barato: { $provider } a { $prompt } / { $completion } por 1M
openrouter-use-cheapest = Usar el más barato
openrouter-compare = Comparar en OpenRouter
openrouter-provider-error = No se pudieron leer los proveedores: { $message }
openrouter-provider-none = OpenRouter no informa ningún proveedor para este modelo.
openrouter-provider = Proveedor
openrouter-add-heading = Añadir OpenRouter
openrouter-add-description = Una clave, todo configurado: el backend más un pool por cada tipo de modelo que sirve (chat, transcription, speech, system_one, embedding, image, rerank), nombrado openrouter-<tipo>. Los pools existentes con ese nombre se unen. La selección de modelos y los límites de precio siguen en el panel de OpenRouter.
openrouter-add-save = Añadir OpenRouter
openrouter-kinds = Tipos de modelo
openrouter-kind-selected-count = { $count } seleccionados
openrouter-kind-catalog-count = { $count } en el catálogo
openrouter-kind-not-enabled = No habilitado
openrouter-key-change = Cambiar clave
openrouter-key-placeholder = Nueva clave API
openrouter-key-save = Guardar clave
openrouter-key-saved = Clave actualizada
openrouter-sort = Ordenar
openrouter-save-changed = Guardar selección · { $count } cambiados
openrouter-dirty-confirm = { $kind } tiene cambios sin guardar. ¿Descartarlos?
openrouter-provider-field = Proveedor
openrouter-provider-openai = Compatible con OpenAI
openrouter-provider-openrouter = OpenRouter
openrouter-provider-hint = OpenRouter lee el catálogo de la cuenta y aplica el límite de precio por modelo en cada solicitud. Los demás proveedores son HTTP compatible con OpenAI.
openrouter-max-price = Máx. $/1M
openrouter-voices = Voces
