# STATUS: llm-generated, unreviewed — pending native-speaker QA

rag-heading = Colecciones RAG

# Toasts — collection CRUD
rag-toast-vanished = La colección desapareció tras guardarse.

# Toasts — refs / sources
rag-toast-bulk-queued-skipped = { $added } fuente(s) en cola; { $skipped } duplicado(s) omitido(s).
rag-toast-bulk-queued = Se puso en cola la indexación de { $added } fuente(s).
rag-toast-reindex-queued-ref = Reindexación de `{ $ref }` puesta en cola.

# Status badges
rag-status-pending = pendiente
rag-status-cloning = clonando
rag-status-indexing = indexando
rag-status-ready = listo
rag-status-error = error

# Collection row
rag-button-edit = Editar
rag-button-add-source = Añadir fuente
rag-button-add-bulk = Añadir fuentes (en bloque)

# Ref / source row
rag-badge-primary = principal
rag-button-reindex = Reindexar
rag-button-set-primary = Establecer como principal
rag-button-remove = Quitar

# Inline per-source editor
rag-label-branch-tag = Rama / etiqueta
rag-button-cancel = Cancelar

# Create-collection form
rag-label-name = Nombre
rag-label-chunk-size = Tamaño del fragmento
rag-label-chunk-overlap = Solapamiento del fragmento

# Edit-collection form
rag-label-description = Descripción

# Embedding model field
rag-label-embedding-model = Modelo de embedding

# Selector de origen + credenciales del proveedor (rag_source.rs). Las
# etiquetas de los campos provienen del proveedor y no se traducen.
rag-label-source-kind = Origen
rag-source-unknown-kind = Tipo de origen desconocido.
rag-source-test-button = Probar conexión
rag-source-test-ok = Conectado como `{ $account }`. { $entries } elemento(s) en la carpeta configurada.
rag-source-test-ok-plain = Conectado. { $entries } elemento(s) en la carpeta configurada.
rag-source-test-failed = No se pudo acceder al origen: { $error }
rag-source-detected = Detectado: { $server }

rag-label-profile = Campos del documento
rag-option-profile-none = Ninguno — indexar solo el texto

# Hook de sincronización — un disparador entrante que resincroniza una colección.
rag-button-sync-token = URL de sync
rag-badge-sync-hook = hook de sync

# Browser consent for an OAuth source (Google Drive).
rag-source-consent-save-first = Guarde primero la colección con su ID de cliente y secreto, luego conéctela para conceder acceso.
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

# Gestor de colecciones de la SPA: formulario de creación, tarjeta de URL de
# sincronización, filas de fuente y las confirmaciones propias de la SPA.
rag-button-new-collection = Nueva colección
rag-label-git-url = URL de Git
rag-source-testing = Probando…
rag-button-create = Crear
rag-button-rebuild = Reconstruir
rag-sync-url-heading = URL de sincronización — se muestra una sola vez
rag-sync-token-confirm = ¿Generar una nueva URL de sincronización? La anterior dejará de funcionar.
rag-delete-collection-confirm = ¿Eliminar la colección { $name } y su índice?
rag-remove-source-confirm = ¿Quitar la fuente { $source }?
rag-toast-rebuild-queued = Reconstrucción completa solicitada.
rag-ref-indexed-at = indexado el { $date }
rag-no-sources = Sin fuentes — esta colección no indexa nada hasta que se añada una.
rag-add-sources-hint = Una fuente por línea; añade { $at } para sustituir la ref { $ref } de esta colección.
