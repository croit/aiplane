# STATUS: llm-generated, unreviewed — pending native-speaker QA

agent-output-withheld = No puedo dar esa respuesta porque mencionaba datos que no pude verificar para usted. Reformule su pregunta o póngase en contacto con el soporte.
agent-output-redacted = [eliminado]
agent-error-name-taken = Ya existe un agente personal con ese nombre en tu cuenta. Elige otro nombre.
agent-error-name-invalid = Introduce un nombre de hasta 80 caracteres y sin caracteres de control.
agent-error-active-limit = Se alcanzó el límite de agentes personales activos. Desactiva uno antes de crear otro.
agent-error-not-found = Ese agente no está disponible para ti.
agent-error-id-missing = Falta el identificador del agente en la solicitud.
agent-memory-error-kind = Elige preferencia, proyecto o hecho como tipo de recuerdo.
agent-memory-error-empty = El contenido del recuerdo no puede estar vacío.
agent-memory-error-id-missing = Falta el identificador del agente o del recuerdo en la solicitud.

# What a website visitor reads next to the secure field when an identity
# verifier (#95) asks for the code it sent. Worded the same whether or not
# the address is registered, so it reveals neither.
agent-verifier-code-sent = Si esta dirección está registrada con nosotros, le hemos enviado un código. Introduzca el código aquí.
agent-verifier-code-again = Introduzca el código que le hemos enviado.

# What a website visitor reads when the public agent endpoint refuses a
# request (#92): the agent's budget is spent, or a rate limit was hit.
agent-embed-unavailable = Este asistente no está disponible temporalmente. Inténtelo de nuevo más tarde o utilice las demás opciones de contacto del sitio web.
agent-embed-rate-limited = Está enviando mensajes más rápido de lo que este asistente los acepta. Espere { $seconds } segundos y vuelva a intentarlo.

agent-embed-decision-for-staff = Esta solicitud está esperando a un miembro de nuestro equipo. Verá la respuesta aquí en cuanto haya decidido.
agent-embed-not-waiting = El asistente no está esperando una respuesta suya en este momento. Vuelva a cargar la conversación.
agent-embed-message-waiting = Su mensaje anterior todavía está esperando al asistente. Espere su respuesta antes de enviar otro.

agent-a2a-input-required = Un servicio asociado necesita un dato más para continuar. Introdúzcalo aquí; solo se envía a ese servicio.
agent-error-inactive = Este agente está inactivo. Actívalo antes de iniciar una conversación.
agent-action-chat = Chatear
agent-action-chat-opening = Abriendo el chat…
agent-activity-session-started = Se inició un chat con el agente.

agent-workspace-error-unavailable = El almacenamiento persistente del agente no está disponible. Pide a tu administrador que revise el servicio sandbox.
agent-workspace-error-conflict = El espacio de trabajo está ocupado o ha cambiado. Recarga y reintenta cuando el agente no esté trabajando.
agent-workspace-error-not-found = No se encontró esta entrada. Recarga el directorio.
agent-workspace-error-too-large = El archivo o directorio supera el límite del espacio de trabajo. Usa uno más pequeño.
agent-workspace-error-invalid = La solicitud no es válida. Comprueba la ruta e inténtalo de nuevo.
agent-workspace-error-failed = La operación ha fallado. Recarga e inténtalo de nuevo.

agent-workspace-heading = Espacio de trabajo
agent-workspace-intro = Los archivos de este espacio de trabajo se conservan entre ejecuciones del agente. Descárguelos para consultarlos.
agent-workspace-reload = Recargar el espacio de trabajo
agent-workspace-unavailable = El almacenamiento persistente no está disponible. Pida a su administrador que revise el servicio sandbox.
agent-workspace-busy = Este agente está usando su espacio de trabajo. Los archivos pueden cambiar. La carga de archivos, la creación de carpetas y la eliminación están disponibles cuando está inactivo.
agent-workspace-empty = Esta carpeta está vacía.
agent-workspace-root = Raíz del espacio de trabajo
agent-workspace-folder = Carpeta
agent-workspace-size-missing = Tamaño no disponible
agent-workspace-modified = Modificado { $when }
agent-workspace-upload = Subir archivo
agent-workspace-file = Archivo
agent-workspace-upload-limit = Seleccione un archivo de hasta 50 MiB.
agent-workspace-create-folder = Crear carpeta
agent-workspace-folder-name = Nombre de la carpeta
agent-workspace-delete = Eliminar
agent-workspace-delete-confirm = ¿Eliminar «{ $name }»? Las carpetas deben estar vacías.
agent-workspace-more = Cargar más archivos
agent-workspace-error-load = No se pudo cargar el espacio de trabajo. Recárguelo e inténtelo de nuevo.
agent-workspace-error-save = No se pudo guardar el cambio. Recargue el espacio de trabajo e inténtelo de nuevo.
agent-workspace-notice-saved = Espacio de trabajo actualizado.
