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
limits-edit = Editar
limits-edit-heading = Editar límite
limits-edit-submit = Guardar cambios
limits-saved = límite guardado para { $subject }
limits-subject-token = Token de API
limits-subject-system = Agente

# Tabla de reglas de la SPA: título, columna «gestionado por» y confirmación.
limits-delete-confirm = ¿Eliminar esta regla?
limits-intro = Limita cuántas solicitudes, cuántos tokens o cuánto gasto puede usar un llamante en una ventana móvil. Las reglas se resuelven de lo más específico primero: gana la regla propia de un usuario, si no, la más generosa de sus roles, si no, el valor predeterminado global. Sin reglas, todos son ilimitados. Una regla sobre un token de API es un tope adicional que se comprueba junto al presupuesto de su propietario, así que solo puede reducir lo que ese token gasta. Solo cuentan los pools medidos (los pools autoalojados con enforce_limits = false están exentos), y todo el presupuesto de un usuario se comparte entre sus tokens de API, el chat y las ejecuciones programadas. Para una llamada a un modelo gana la regla coincidente más específica con todas sus ventanas —primero por sujeto, luego por alcance de modelo (exacto antes que una familia `prefix-*`, y esta antes que «todos los modelos»)— y el gasto solo cuenta para la regla que ganó. Una regla marcada como tope general limita además todo lo que cubre. Los límites de coste nunca bloquean un modelo con precio cero.
limits-field-subject-id = Rol / usuario / token / agente
limits-field-subject-id-ph = Elija …
limits-col-value = Valor
limits-col-actions = Acciones
limits-deleted = límite eliminado
limits-field-total = Tope general
limits-field-total-hint = Si se marca, esta regla limita además todo lo que cubre, incluso donde una regla más específica permite más. Si no, solo se aplica donde es la regla coincidente más específica.
limits-field-model-hint = Vacío significa todos los modelos; un * final designa una familia (p. ej. claude-*).
limits-col-kind = Tipo
limits-kind-total = General
limits-kind-override = Excepción
