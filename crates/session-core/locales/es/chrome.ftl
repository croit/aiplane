# STATUS: llm-generated, unreviewed — pending native-speaker QA

chrome-theme-toggle-title = Cambiar tema
chrome-theme-toggle-aria-to-light = Cambiar a tema claro
chrome-theme-toggle-aria-to-dark = Cambiar a tema oscuro
chrome-lang-switcher-aria = Elegir idioma

# Web Push turn-complete notifications (server-sent body; `spawn_assistant_worker`).
push-untitled-conversation = Nueva conversación
push-turn-complete-body = Tu respuesta está lista.
push-turn-error-body = El turno terminó con un error.

# Web Push: la autorización de un conector ha muerto (barrido de refresco
# proactivo, `tools::mcp::worker`). { $connector } es el nombre visible.
push-connector-reconnect-title = La conexión necesita tu inicio de sesión
push-connector-reconnect-body = { $connector } se desconectó: abre Integraciones para reconectar.

# SPA-only: the root route, which only routes on to /chat.
chrome-opening-conversations = Abriendo tus conversaciones…

# SPA-only: /login, which exists to bounce straight to the IdP.

searchable-select-search-placeholder = Buscar opciones…
searchable-select-search-aria = Buscar en { $field }
searchable-select-clear-search = Borrar búsqueda
searchable-select-no-results = No hay opciones coincidentes.
searchable-select-model-gdpr = GDPR
searchable-select-model-nda = NDA

# Se muestra en lugar de una página cuya función opcional se ha desactivado
# en /admin/settings. En ese caso la entrada de navegación desaparece; esto
# responde a un enlace antiguo o a una URL escrita a mano.
feature-disabled-body = { $feature } está desactivado en esta pasarela, así que esta página no tiene nada que mostrar. Un administrador puede activarlo en Ajustes.
feature-disabled-settings-link = Abrir ajustes

multi-select-none = Nada seleccionado
multi-select-count = { $count } seleccionados
multi-select-clear = Borrar todo
multi-select-unknown = No registrado aquí: elimínelo o créelo
multi-select-wildcard-tools = Todas las herramientas, incluidas las que se añadan después
multi-select-wildcard-skills = Todas las habilidades, incluidas las que se añadan después
multi-select-shadowed = Cubierto por ✱
multi-select-wildcard-warning = ✱ concede también, sin revisión, las herramientas que añada una versión futura. En un grupo que no sea de administración, es preferible enumerar las herramientas concretas.
multi-select-family-comfyui = Todos los flujos de ComfyUI, incluidos los que se añadan después
multi-select-family-mcp = Todas las herramientas de { $subject }, incluidas las que se añadan después
multi-select-mcp-none-cached = Aún no se conocen herramientas de este conector: se registran la primera vez que alguien lo conecta. Conceda el conector completo arriba o vuelva cuando se haya usado.
