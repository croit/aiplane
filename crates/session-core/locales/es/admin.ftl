# Strings owned by `gateway/src/rama_server/pages/admin.rs` — la página
# `/admin/models`.

admin-heading = Modelos

admin-col-model = Modelo

admin-not-configured = sin configurar

admin-badge-ctx = CTX

admin-save-model = Guardar modelo
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
admin-comfyui-reloaded = Se recargaron { $count } flujo(s) de trabajo.
admin-comfyui-empty = No hay flujos de trabajo cargados — revisa el directorio de contenido.
admin-defaults-model-aria = Modelo predeterminado para { $feature }
admin-defaults-set = Establecer
admin-search-provider-none = Ninguno
admin-add-overrides-heading = Añadir anulaciones de modelo
admin-edit-model-heading = Editar { $model }
admin-add-model = Añadir…
admin-pricing-unit-label = Unidad de precio
admin-pricing-unit-mtok = por Mtok
admin-pricing-unit-ktok = por Ktok
admin-pricing-unit-kimgs = por 1000 imágenes
admin-clear-overrides-confirm = ¿Descartar todas las anulaciones guardadas de { $model }?
admin-users-col-email = Correo electrónico
