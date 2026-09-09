# STATUS: llm-generated, unreviewed — pending native-speaker QA

backends-page-title = Backends de origen — LLM Gateway
backends-heading = Backends de origen
backends-description-prefix = Vista en vivo de los pools de origen configurados — estado, carga en curso frente al límite de cada backend y los modelos que cada uno ofrece actualmente. Solo lectura: el enrutamiento depende por completo de lo que los backends informan en su
backends-description-suffix = sonda.
backends-summary = { $total } backends · { $healthy } saludables · { $down } caídos
backends-unknown-fallback-prefix = Alternativa para modelo desconocido —
backends-empty-prefix = No hay pools de origen configurados. Añada un bloque
backends-empty-suffix = a gateway.toml y reinicie.

backends-fallback-offline-title = fallback_offline: se usa cuando todos los backends de un modelo conocido en este pool están caídos
backends-fallback-offline-badge = fuera de línea ↩ { $model }
backends-pool-empty = No hay backends en este pool.

backends-status-down = caído
backends-status-saturated = saturado
backends-status-up = activo

backends-inflight-label = en curso { $load }
backends-activity-summary = 15m { $m15 } · 30m { $m30 } · 60m { $m60 }
backends-no-models = no se anuncian modelos
backends-aliases-label = alias:

backends-alias-target-title = alias → { $target }
backends-alias-disabled-label = { $name } (desactivado)
backends-alias-disabled-title = alias simple desactivado — este backend sirve varios modelos; asígnele un destino explícito (formulario de mapeo)
backends-alias-bare-title = alias → modelo de este backend

# Editor CRUD de backends (añadir/editar/eliminar backends almacenados en la topología de la base de datos).
backends-manage-heading = Gestionar backends
backends-manage-description = Añada, edite o elimine backends de origen. Los cambios se guardan en la base de datos, pero solo surten efecto una vez que haga clic en «Aplicar cambios».
backends-apply-changes = Aplicar cambios
backends-add-heading = Añadir backend
backends-field-name = Nombre
backends-field-base-url = URL base
backends-field-api-key-env = Variable de entorno de clave de API
backends-field-health-path = Ruta de estado
backends-field-weight = Peso
backends-field-max-inflight = Máximo en curso
backends-field-pool = Grupo
backends-field-pool-none = (ninguno)
backends-field-pool-hint = Asigna este backend a un grupo. Un backend en varios grupos se reduce al elegido aquí.
backends-field-models = Modelos (separados por comas)
backends-field-aliases = Alias (name=target por línea)
backends-field-probe-models = Descubrir modelos mediante la sonda /models
backends-field-supports-edit = Admite edición de imágenes
backends-save-backend = Guardar backend
backends-add-backend = Añadir backend
backends-delete-backend = Eliminar
backends-error-name-required = el nombre del backend es obligatorio
backends-error-base-url-required = la URL base es obligatoria
backends-saved = backend `{ $name }` guardado — haga clic en «Aplicar cambios» para recargar
backends-deleted = backend `{ $name }` eliminado — haga clic en «Aplicar cambios» para recargar

backends-field-api-key = Clave API
backends-field-api-key-placeholder = Clave API (almacenada cifrada)
backends-field-api-key-keep = dejar en blanco para conservar la clave actual

# Duplicate-name guard on the Add-backend form.
backends-error-name-exists = ya existe un backend llamado `{ $name }` — haga clic de nuevo en «Añadir backend» para sobrescribirlo, o cambie el nombre
backends-overwrite-hint = Este nombre ya existe. Volver a guardar SOBRESCRIBE el backend existente: su URL base, clave API, modelos, alias y asignación de pool. Cambie el nombre para añadir un segundo backend.

# An alias that is configured but would not route.
backends-alias-unresolved-title = ROTO: este alias apunta a `{ $target }`, que este backend no sirve, así que las peticiones fallan.
backends-alias-nothing-title = ROTO: este backend no anuncia ningún modelo, así que un alias simple no tiene a qué vincularse.
backends-alias-serves = Sirve: { $models }
backends-alias-serves-nothing = Actualmente no sirve ningún modelo.
# Save-time check on the aliases textarea.
backends-alias-target-unknown = guardado, pero este backend no sirve estos destinos de alias: { $targets }; sirve { $models }. Esos alias no enrutarán hasta que el destino coincida exactamente.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
backends-auth-failed = clave rechazada
backends-auth-failed-title = El upstream rechazó las credenciales de la sonda de salud (401/403), así que el descubrimiento de modelos está desactivado y nada nuevo puede enrutarse por este backend. Revisa la clave API; si usa una variable de entorno, comprueba que esté definida.
backends-no-models-title = Este backend no anuncia ningún modelo, así que nada se enruta ahí y un alias simple no tiene a qué vincularse. Normalmente una sonda que nunca devolvió datos.
# Maintenance switch.
backends-enabled-label = Acepta tráfico
backends-enabled-hint = Desactiva para vaciar este backend por mantenimiento. Surte efecto de inmediato: no hace falta «Aplicar cambios». Sus modelos siguen siendo conocidos, así que los otros backends del pool asumen la carga y los clientes ven una interrupción temporal, nunca «modelo no encontrado».
backends-enabled-on = el backend `{ $name }` vuelve a aceptar tráfico
backends-enabled-off = backend `{ $name }` vaciado por mantenimiento: no se enrutarán nuevas peticiones
backends-status-drained = mantenimiento
backends-status-drained-title = Vaciado por mantenimiento: el backend es alcanzable pero el router lo omite. Vuelve a activar «Acepta tráfico» para devolverlo a la rotación.
# "Test connection": call the upstream with what is typed in the editor.
backends-test-button = Probar conexión
backends-test-hint = Llama a esta URL con las credenciales de arriba. No se guarda nada.
backends-test-insert-hint = Ids de modelos anunciados: haz clic para completar la línea de alias donde está el cursor:
backends-test-ok = Alcanzable, autenticado ({ $source }), { $count } modelos descubiertos.
backends-test-ok-no-models = Alcanzable y autenticado ({ $source }), pero la respuesta no es un envoltorio /models de OpenAI, así que el descubrimiento no puede leerla. Este backend solo podrá servir los ids que indiques en «Modelos».
backends-test-auth-failed = Rechazado con HTTP { $status }: la credencial fue denegada ({ $source }). Hasta que se corrija, el descubrimiento de modelos queda desactivado y el backend no anuncia nada.
backends-test-http-error = { $url } respondió HTTP { $status }.
backends-test-unreachable = No se pudo alcanzar { $url }: { $err }
backends-test-timeout = { $url } no respondió en { $secs }s.
backends-test-key-typed = con la clave escrita arriba
backends-test-key-stored = con la clave almacenada
backends-test-key-env = desde env { $var }
backends-test-key-env-unset = env { $var } NO ESTÁ DEFINIDA: la petición salió sin credencial
backends-test-key-none = sin credencial enviada
# Where the API key comes from (U5).
backends-key-env-badge = clave: env { $var }
backends-key-env-title = Este backend no tiene clave almacenada; la lee de esta variable de entorno, que está definida.
backends-key-env-unset-badge = env { $var } NO DEFINIDA
backends-key-env-unset-title = Este backend no tiene clave almacenada y la variable de entorno que nombra no está definida en el proceso del gateway, así que no envía ninguna credencial. Si el upstream la exige, cada sonda recibe 401, el descubrimiento de modelos queda desactivado y el backend no anuncia nada. Introduce la clave en el campo «Clave API», o define la variable y reinicia.
# Live name-clash note on the add form (U9).
backends-name-taken = Ya existe un backend con este nombre: guardar lo sobrescribiría, incluidos su URL base, clave, modelos y pool. Elige otro nombre para añadir un segundo backend.

# Filas de backend de la SPA: interruptor de drenaje, carga por hora y borrado.
backends-drain-button = Drenar
backends-undrain-button = Reactivar
backends-requests-per-hour = { $count } sol./h
backends-delete-confirm = ¿Eliminar el backend { $name }? Aplica la topología después.
