# STATUS: llm-generated, unreviewed — pending native-speaker QA

tools-heading = Herramientas
tools-description = Activa o desactiva las herramientas que el asistente puede usar. Los cambios se aplican solo a tu cuenta y surten efecto en tu próximo mensaje.
tools-none-granted = Tus roles no conceden ninguna herramienta.

tools-location-heading = Ubicación
tools-location-description = Comparte la ubicación precisa de tu dispositivo para que el asistente pueda responder preguntas como «¿qué tiempo hace aquí?». Solo se usa para tus llamadas a herramientas y puedes dejar de compartirla en cualquier momento. Sin ella, el asistente recurre a una ubicación aproximada derivada de tu dirección IP.
tools-location-share-button = Compartir ubicación precisa
tools-location-stop-button = Dejar de compartir
tools-location-shared = Compartida.
tools-location-shared-accuracy = Compartida — precisión ±{ $accuracy } m.
tools-location-not-shared = No compartida.
tools-location-unavailable = No se pudo acceder a tu ubicación. Comprueba el permiso de ubicación del navegador e inténtalo de nuevo.

# SPA-only: the Svelte /tools toggle list.
tools-toggle-aria = Activar o desactivar { $name }
tools-no-description = Sin descripción

# Section headings for the tool catalog, shared by /tools, the per-token
# capability panel, the chat capability picker and the admin grant matrix.
tool-category-web-network = Web y Red
tool-category-attachments-documents = Adjuntos y Documentos
tool-category-document-templates = Plantillas de documentos
tool-category-knowledge-base = Base de conocimiento
tool-category-code-sandbox = Código y Sandbox
tool-category-memory = Memoria
tool-category-integrations = Integraciones
tool-category-utility = Utilidades
tool-category-skills = Skills
tool-category-images-media = Imágenes y Medios
tool-category-comfyui-workflows = Flujos de ComfyUI
tool-category-scheduled-actions = Acciones programadas

tools-configure-link = Configurar
tools-needs-storage = Se necesita almacenamiento de archivos.
tools-needs-rag = Se necesita una base de conocimientos.
tools-needs-push = Se necesitan notificaciones push.
tools-needs-geoip = Se necesita una base de datos GeoIP.
tools-needs-image-backend = Se necesita un backend de imágenes.
tools-needs-sandbox = Se necesita un ejecutor sandbox.
tools-needs-sandbox-network = Se necesita acceso de red para el sandbox.

# SPA-only: /tools/browser, setting up the Chrome extension browser_control drives.
tools-browser-tab = Extensión del navegador
tools-browser-description = Permite que una conversación actúe en este navegador, con tus sesiones iniciadas, en páginas a las que solo tú puedes acceder: una herramienta interna, una página tras un inicio de sesión único, un formulario que solo tú puedes enviar. Solo funciona mientras la conversación está abierta y después de activar la extensión.
tools-browser-status-heading = Estado
tools-browser-status-not-granted = Tus roles no permiten el control del navegador. Pídeselo a tu administrador si lo necesitas.
tools-browser-status-tool-off = El control del navegador está desactivado en tu lista de herramientas.
tools-browser-status-not-detected = Ninguna extensión respondió en esta página. Instálala y añade este AIplane en su configuración.
tools-browser-status-switched-off = La extensión está instalada y vinculada, pero desactivada.
tools-browser-status-ready = Listo. El asistente puede actuar en este navegador.
tools-browser-switch-on = Activar
tools-browser-switch-on-fallback = Chrome no abrió la ventana de la extensión. Haz clic en el icono de la extensión en la barra de herramientas y elige Activar.
tools-browser-open-tools = Abrir la lista de herramientas
tools-browser-recheck = Comprobar de nuevo
tools-browser-install-heading = Instalar
tools-browser-store-button = Chrome Web Store
tools-browser-download-button = Descargar .zip
tools-browser-download-note = El .zip sirve para instalarla sin empaquetar: en Linux o en el modo de desarrollador de Chrome. En Windows y macOS, Chrome solo instala extensiones desde la tienda.
tools-browser-steps-heading = Configuración
tools-browser-step-install = Instala la extensión desde la Chrome Web Store. Necesita Chrome 127 o posterior.
tools-browser-step-pair = Abre la configuración de la extensión y añade la dirección de este AIplane. Chrome pedirá permiso para esa dirección; al permitirlo, la vinculación queda completa.
tools-browser-step-switch-on = Vuelve a esta página y haz clic en Activar, o usa el icono de la extensión en la barra de herramientas. La primera vez, Chrome pide acceso a los sitios web.
tools-browser-step-ask = Pídelo en una conversación, por ejemplo: «Abre nuestra wiki interna y resume la página de incorporación».
tools-browser-unpacked-heading = Instalar sin empaquetar
tools-browser-unpacked-steps = Descomprime la descarga, abre chrome://extensions, activa el modo de desarrollador, haz clic en «Cargar descomprimida» y elige la carpeta descomprimida.
tools-browser-origin-label = Dirección de este AIplane
tools-browser-copy-origin = Copiar
tools-browser-copied = Copiado
tools-browser-notes-heading = Conviene saber
tools-browser-note-window = El asistente trabaja en su propia ventana, en un grupo de pestañas llamado «Assistant». Mientras la extensión está activa, su icono es verde y Chrome muestra una barra indicando que el navegador se está depurando.
tools-browser-note-open = Solo funciona mientras la conversación está abierta en una pestaña. Desactiva la extensión desde su icono cuando termines.
tools-browser-note-injection = Las páginas que lee el asistente pueden contener texto dirigido a él. En la configuración de la extensión puedes limitarla a los sitios que apruebes.

tool-forward-request-title = Preguntar a otro agente
tool-forward-request-description = Envía una tarea explícita a otro agente personal. Ambos agentes deben permitir la solicitud; el agente receptor utiliza sus propias herramientas y reglas de aprobación.
