# STATUS: llm-generated, unreviewed — pending native-speaker QA


pools-field-name = Nombre
pools-field-kind = Tipo
pools-field-models = Modelos servidos (lista de permitidos, separados por comas)
pools-field-backends = Backends
pools-save-pool = Guardar pool
pools-delete-pool = Eliminar

# Tarjetas de pool de la SPA: los dos resúmenes de una línea y el borrado.
pools-add-heading = Añadir pool
pools-fallbacks-heading = Alternativas para modelos desconocidos
pools-fallbacks-description = Cuando una solicitud nombra un modelo que el gateway nunca ha conocido, sustitúyalo por este modelo para ese tipo. En blanco = el fallo devuelve 404.
upstreams-problems-heading = Este pool no funciona por completo
upstreams-coverage-heading = Lo que ven los clientes
upstreams-coverage-hint = Exactamente los nombres que GET /v1/models devuelve para este pool, cada uno con cuántos de sus backends pueden servirlo ahora. Cualquier cosa por debajo de completo significa que parte de tu hardware está inactivo para ese nombre.
upstreams-coverage-none-title = Ningún backend puede servir este nombre ahora mismo. Las peticiones reciben un error de interrupción temporal.
upstreams-coverage-partial-title = Solo algunos backends sirven este nombre: las peticiones usan parte del pool y el resto queda inactivo. Normalmente un alias cuyo destino no coincide con lo que anuncia un backend.
upstreams-coverage-full-title = Todos los backends de este pool sirven este nombre.
pools-name-taken = Ya existe un pool con este nombre: guardar lo reemplazaría, incluidos sus backends y modelos. Elige otro nombre para crear un pool nuevo.
pools-field-strategy = Estrategia
pools-field-strategy-hint = prefix_affinity mantiene una conversación en la réplica que ya tiene su caché KV (lo mejor para tráfico de chat/agente repartido entre varias GPU: las otras alternan turnos entre réplicas y pagan un prefill completo cada vez), y aun así reparte cuando un backend está realmente más cargado. least_inflight equilibra por carga actual; round_robin rota por peso.
pools-field-fallback-offline = Modelo alternativo fuera de línea
pools-field-fallback-offline-placeholder = servido cuando todos los backends están caídos
pools-field-models-hint = Si se define, solo se sirven estos ids de un backend con sondeo /models; el resto se muestra tachado. En blanco = servir todo lo que informa el backend.
pools-field-allowed-groups = Grupos permitidos
pools-field-allowed-groups-hint = Grupos del gateway (separados por comas) autorizados a ver y usar los modelos de este pool. VacÃ­o = todos. Los admins siempre tienen acceso. Gestiona los grupos en Admin â Grupos.
pools-field-voices = Voces (lang=voice por línea)
pools-field-offer-voices = Voces seleccionables (una por línea, elige el usuario)
pools-no-backends = Aún no hay backends definidos. Añada uno primero en la página de Backends.
pools-field-gdpr = Conforme al GDPR
pools-field-nda = Cubierto por NDA
pools-field-enforce-limits = Aplicar límites de tasa y cuotas

upstreams-problem-no-backends = Sin backends asignados: nada en este pool puede atender una petición.
upstreams-problem-all-drained = Todos los backends están vaciados por mantenimiento, no se enruta nada aquí.
upstreams-problem-all-down = Ningún backend disponible: las peticiones esperan a que vuelva uno y luego reciben un error de interrupción temporal.
upstreams-problem-auth = Credencial rechazada por: { $backends }. Ahí el descubrimiento de modelos está desactivado y no anuncian nada.
upstreams-problem-no-models = Sin modelos anunciados por: { $backends }. Nada se enruta a ellos, y un alias simple no tiene a qué vincularse.
upstreams-problem-broken-aliases = Alias que no enrutan a ninguna parte: { $aliases }. Cada uno apunta a un modelo que su backend no sirve, o no tiene a qué vincularse.
upstreams-problem-partial-coverage = Servidos solo por parte del pool: { $models }. Las peticiones a estos nombres usan menos réplicas de las que tienes.
upstreams-problem-unserved-allowlist = Listados en Modelos pero servidos por nadie: { $models }.
upstreams-problem-missing-backends = Backends asignados que ya no existen: { $backends }.
upstreams-apply-diff-summary = Ver qué cambiará al aplicar
upstreams-diff-pool-added = el nuevo pool { $pool } entra en servicio
upstreams-diff-pool-removed = el pool { $pool } deja de servir
upstreams-diff-pool-kind = pool { $pool }: tipo { $from } → { $to }
upstreams-diff-pool-strategy = pool { $pool }: estrategia { $from } → { $to }
upstreams-diff-backend-joins = { $backend } entra en el pool { $pool } y empieza a recibir tráfico
upstreams-diff-backend-leaves = { $backend } sale del pool { $pool } y deja de recibir tráfico
upstreams-diff-backend-url = { $backend }: URL base { $from } → { $to } (sus modelos descubiertos se vuelven a sondear)
upstreams-diff-backend-limits = { $backend }: peso { $weight }, máx. en vuelo { $inflight }
upstreams-diff-backend-health-path = { $backend }: ruta de salud → { $to }
