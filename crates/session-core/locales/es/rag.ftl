# STATUS: llm-generated, unreviewed — pending native-speaker QA

rag-heading = Colecciones RAG
rag-description-prefix = Bases de código que la pasarela ha indexado. La herramienta
rag-description-suffix = consulta estas colecciones para responder preguntas sobre el código.
rag-collections-heading = Colecciones configuradas
rag-empty-list = Aún no hay colecciones. Cree una arriba.

# Toasts — collection CRUD
rag-toast-indexing-queued = Se puso en cola la indexación de `{ $name }` @ `{ $ref }`.
rag-toast-created-aggregate = `{ $name }` creada (agregado). Añada los repositorios de origen abajo para indexarlos.
rag-toast-collection-saved = `{ $name }` guardada.
rag-toast-vanished = La colección desapareció tras guardarse.
# Toasts — refs / sources
rag-toast-bulk-queued-skipped = { $added } fuente(s) en cola; { $skipped } duplicado(s) omitido(s).
rag-toast-bulk-queued = Se puso en cola la indexación de { $added } fuente(s).
rag-toast-source-updated = Fuente actualizada.

# Status badges
rag-status-pending = pendiente
rag-status-cloning = clonando
rag-status-indexing = indexando
rag-status-ready = listo
rag-status-error = error

# Collection row
rag-pat-set = PAT establecido
rag-pat-none = sin PAT
rag-meta-aggregate = { $count } fuente(s) · { $hint }
rag-meta-versioned = { $url } · { $hint }
rag-badge-aggregate = agregado
rag-embed-prefix = embed:
rag-button-edit = Editar
rag-button-delete-collection = Eliminar colección
rag-placeholder-source-git-url = https://github.com/org/repo.git
rag-button-add-source = Añadir fuente
rag-placeholder-branch-tag-commit = rama, etiqueta o commit
rag-button-add-ref = Añadir referencia
rag-placeholder-bulk-sources = Añadir en bloque — un repositorio por línea, @ref opcional:
    https://github.com/proxmox/pve-manager.git
    https://github.com/proxmox/qemu-server.git @master
rag-button-add-bulk = Añadir fuentes (en bloque)

# Ref / source row
rag-badge-primary = principal
rag-ref-indexed-line = indexado { $date } · { $commit }
rag-never = nunca
rag-button-log = Registro
rag-button-reindex = Reindexar
rag-button-set-primary = Establecer como principal
rag-button-remove = Quitar

# Indexing log
rag-log-info = info
rag-log-warn = aviso
rag-log-error = error
rag-log-heading = Registro de indexación
rag-log-empty = Aún no se han registrado eventos de indexación. La primera ejecución se registrará aquí en cuanto el indexador procese esta referencia.

# Inline per-source editor
rag-label-git-url-source = URL de Git (esta fuente)
rag-label-git-url-inherit = URL de Git (vacío = heredar de la colección)
rag-placeholder-git-url = https://example.com/org/repo.git
rag-label-branch-tag = Rama / etiqueta
rag-button-save-source = Guardar fuente
rag-button-cancel = Cancelar

# Create-collection form
rag-create-heading = Indexar una nueva colección
rag-create-description = El indexador clona el repositorio, divide cada archivo en fragmentos y los convierte en embeddings con el modelo configurado. Los PAT se almacenan tal cual (la pasarela se ejecuta en infraestructura de confianza).
rag-new-page-title = Indexar una colección nueva
rag-edit-page-title = Editar colección
rag-not-found = No hay ninguna colección con ese id. Es posible que se haya eliminado.
rag-back-to-collections = Colecciones RAG
rag-edit-source-heading = Editar fuente
rag-add-source-heading = Añadir una fuente
rag-label-name = Nombre
rag-placeholder-name = p. ej. gateway-repo
rag-label-description-optional = Descripción (opcional)
rag-placeholder-description = breve y legible
rag-label-git-url-versioned = URL de Git (solo versionado)
rag-label-pat-optional = Token de acceso personal (opcional)
rag-placeholder-pat = para repositorios privados
rag-label-include-globs-full = Patrones de inclusión (separados por comas o saltos de línea)
rag-placeholder-include-globs = *.rs, *.md
rag-label-exclude-globs = Patrones de exclusión
rag-placeholder-exclude-globs = target/, node_modules/
rag-label-chunk-size = Tamaño del fragmento
rag-label-chunk-overlap = Solapamiento del fragmento
rag-label-refresh-interval = Resincronización automática
rag-hint-refresh-interval = Con qué frecuencia se reindexa sin que nadie lo pida. Una fuente que no puede avisar al gateway —un archivo de lista de correo, un recurso WebDAV sencillo— solo está tan actualizada como esto.
rag-refresh-never = Nunca (manual o webhook de sincronización)
rag-refresh-hourly = Cada hora
rag-refresh-daily = Cada día
rag-refresh-weekly = Cada semana
rag-refresh-custom = Cada { $mins } minutos
rag-create-aggregate-help = Agregado (multi-fuente): busca en muchos repositorios como un único corpus. Deje la URL de Git vacía y añada cada repositorio de origen después de crear la colección. La rama / etiqueta se convierte en la referencia predeterminada de las fuentes añadidas.
rag-button-queue-indexing = Programar indexación

# Edit-collection form
rag-edit-heading = Editando { $name }
rag-label-description = Descripción
rag-label-pat = Token de acceso personal
rag-placeholder-pat-keep = deje en blanco para conservar el existente
rag-label-clear-pat = Eliminar el PAT guardado (dejar de autenticarse)
rag-label-include-globs = Patrones de inclusión
rag-button-save-changes = Guardar cambios

# Embedding model field
rag-label-embedding-model = Modelo de embedding
rag-placeholder-embedding-model-none = no hay pools de embedding configurados — escriba un id de modelo
rag-option-choose-embedding-model = Elija un modelo de embedding…
rag-suffix-not-advertised = (ya no disponible)

rag-label-allowed-groups = Grupos permitidos
rag-hint-allowed-groups = Grupos del gateway (separados por comas) autorizados a listar y buscar en esta colecciÃ³n. VacÃ­o = todos los que tengan las herramientas RAG. Los admins siempre tienen acceso.

# Selector de origen + credenciales del proveedor (rag_source.rs). Las
# etiquetas de los campos provienen del proveedor y no se traducen.
rag-label-source-kind = Origen
rag-source-git-help = Clona un repositorio e indexa sus archivos. El comportamiento original.
rag-source-secret-placeholder = dejar vacío para conservar el valor guardado
rag-source-unknown-kind = Tipo de origen desconocido.
rag-source-test-button = Probar conexión
rag-source-test-ok = Conectado como `{ $account }`. { $entries } elemento(s) en la carpeta configurada.
rag-source-test-ok-plain = Conectado. { $entries } elemento(s) en la carpeta configurada.
rag-source-test-failed = No se pudo acceder al origen: { $error }
rag-source-test-git = Elige un origen remoto para probar. Los repositorios Git se comprueban al indexar.
rag-source-detected = Detectado: { $server }

rag-label-profile = Campos del documento
rag-option-profile-none = Ninguno — indexar solo el texto
rag-profile-help = Extrae campos (proveedor, fecha, importe, proyecto) de cada documento para poder filtrarlos, ordenarlos y sumarlos. Cuesta una llamada al modelo por documento; deja «Ninguno» para colecciones de código o texto plano.

# Editor de perfiles de extracción (/rag/profiles, rag_profiles.rs)
rag-profile-heading = Perfiles de extracción
rag-profile-description = Lo que se extrae de cada documento de una colección: los campos que hacen que «la última factura de X» o «cuánto gastamos» tengan respuesta. Un perfil se asigna a una colección desde la página RAG.
rag-profile-create-heading = Nuevo perfil
rag-profile-list-heading = Perfiles
rag-profile-empty = Todavía no hay perfiles.
rag-profile-builtin = incluido
rag-profile-version = v{ $version }
rag-profile-summary = { $count } campo(s)
rag-profile-label-name = Nombre
rag-profile-label-description = Descripción
rag-profile-label-prompt = Instrucciones de extracción
rag-profile-label-fields = Campos (JSON)
rag-profile-prompt-placeholder = Describe qué está leyendo el modelo y cómo normalizar fechas e importes.
rag-profile-fields-help = Un objeto por campo: key, label, type (text | number | date | enum), description y, opcionalmente, filterable / sortable. Un enum necesita además «values». La descripción se le muestra al modelo, así que sé preciso.
rag-profile-edit-warning = Al guardar se incrementa la versión del perfil y se vacía su caché de extracción. Las colecciones que lo usan deben reindexarse para adoptar los nuevos campos.
rag-profile-button-create = Crear perfil
rag-profile-button-save = Guardar
rag-profile-button-delete = Eliminar
rag-profile-delete-confirm = ¿Eliminar el perfil { $name }?
rag-profile-example-counterparty-label = Contraparte
rag-profile-example-counterparty-description = La otra parte.
rag-profile-example-date-label = Fecha
rag-profile-example-date-description = La fecha del documento.
rag-profile-example-amount-label = Importe
rag-profile-example-amount-description = El importe total.
rag-profile-link = Editar perfiles de extracción
rag-profile-toast-created = Perfil «{ $name }» creado.
rag-profile-toast-saved = «{ $name }» guardado.
rag-profile-toast-saved-reindex = «{ $name }» guardado. Reindexa para aplicarlo: { $collections }.
rag-profile-toast-deleted = Perfil eliminado.
# Hook de sincronización — un disparador entrante que resincroniza una colección.
rag-toast-sync-token = URL de sincronización (se muestra una sola vez, no se almacena): { $url }
rag-toast-sync-token-cleared = URL de sincronización desactivada.
rag-button-sync-token = URL de sync
rag-button-sync-token-rotate = Nueva URL de sync
rag-button-sync-token-clear = Desactivar la URL de sync
rag-badge-sync-hook = hook de sync

# Browser consent for an OAuth source (Google Drive).
rag-source-consent-save-first = Guarde primero la colección con su ID de cliente y secreto, luego conéctela para conceder acceso.
rag-source-consent-connected = conectada
rag-source-consent-connect = Conectar
rag-oauth-lookup-failed = No se pudo leer la colección.
rag-oauth-not-oauth = Este tipo de fuente no se conecta en el navegador.
rag-oauth-no-client = Guarde primero el ID de cliente y el secreto de OAuth en la colección.
rag-oauth-bad-authorize-url = No se pudo construir la URL de autorización del proveedor.
rag-oauth-start-failed = No se pudo iniciar la autorización.
rag-oauth-callback-missing = Faltaba el código o el estado en la respuesta del proveedor.
rag-oauth-expired = Esa autorización caducó o ya se usó. Vuelva a empezar.
rag-oauth-provider-refused = El proveedor rechazó la autorización: { $error }
rag-oauth-exchange-failed = Falló el intercambio del código de autorización: { $error }
rag-oauth-no-refresh-token = El proveedor no devolvió un token de actualización, por lo que la indexación desatendida no sería posible. Revoque el acceso de la pasarela en su cuenta del proveedor y vuelva a conectar.
rag-oauth-store-failed = No se pudieron guardar las credenciales.
rag-badge-no-files = sin archivos indexados
rag-ref-files = { $files } archivos
rag-label-git-url = URL de Git
rag-source-testing = Probando…
rag-sync-url-heading = URL de sincronización — se muestra una sola vez
rag-sync-token-confirm = ¿Generar una nueva URL de sincronización? La anterior dejará de funcionar.
rag-delete-collection-confirm = ¿Eliminar la colección { $name } y su índice?
rag-remove-source-confirm = ¿Quitar la fuente { $source }?
rag-toast-rebuild-queued = Reconstrucción completa solicitada.
rag-no-sources = Sin fuentes — esta colección no indexa nada hasta que se añada una.
rag-add-sources-hint = Una fuente por línea; añade { $at } para sustituir la ref { $ref } de esta colección.
