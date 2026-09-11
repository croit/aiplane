# Editor de administración de límites de tasa / cuotas (/admin/limits).
limits-heading = Límites de tasa y cuotas
limits-add-heading = Añadir o actualizar un límite
limits-field-subject = Se aplica a
limits-field-model = Modelo
limits-field-dimension = Límite
limits-field-window = Por
limits-field-value = Valor
limits-add-submit = Guardar límite
limits-subject-global = Todos (predeterminado)
limits-subject-role = Rol
limits-subject-user = Usuario
limits-dim-requests = Solicitudes
limits-dim-tokens = Tokens
limits-dim-cost = Coste ({ $cur })
limits-dim-cost-short = Coste
limits-win-hour = Hora
limits-win-day = Día
limits-win-week = Semana
limits-win-month = Mes
limits-col-subject = Se aplica a
limits-col-scope = Modelo
limits-col-limit = Límite
limits-col-window = Ventana
limits-none = No hay límites configurados — todos son ilimitados.
limits-all-models = todos los modelos
limits-delete = Eliminar
limits-saved = límite guardado para { $subject }
limits-subject-token = Token de API

# Tabla de reglas de la SPA: título, columna «gestionado por» y confirmación.
limits-delete-confirm = ¿Eliminar esta regla?
limits-intro = Limita cuántas solicitudes, cuántos tokens o cuánto gasto puede usar un llamante en una ventana móvil. Las reglas se resuelven de lo más específico primero: gana la regla propia de un usuario, si no, la más generosa de sus roles, si no, el valor predeterminado global. Sin reglas, todos son ilimitados. Una regla sobre un token de API es un tope adicional que se comprueba junto al presupuesto de su propietario, así que solo puede reducir lo que ese token gasta. Solo cuentan los pools medidos (los pools autoalojados con enforce_limits = false están exentos), y todo el presupuesto de un usuario se comparte entre sus tokens de API, el chat y las ejecuciones programadas.
limits-field-subject-id = Rol / usuario / token
limits-field-subject-id-ph = id de rol, correo del usuario o id de token
limits-col-value = Valor
limits-col-actions = Acciones
limits-deleted = límite eliminado
