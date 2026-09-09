# STATUS: llm-generated, unreviewed — pending native-speaker QA

backends-heading = Backends de origen

backends-status-down = caído
backends-status-up = activo

backends-inflight-label = en curso { $load }

# Editor CRUD de backends (añadir/editar/eliminar backends almacenados en la topología de la base de datos).
backends-apply-changes = Aplicar cambios
backends-field-name = Nombre
backends-field-base-url = URL base
backends-field-pool = Grupo
backends-field-pool-none = (ninguno)
backends-save-backend = Guardar backend
backends-add-backend = Añadir backend
backends-delete-backend = Eliminar

backends-field-api-key = Clave API
backends-field-api-key-keep = dejar en blanco para conservar la clave actual

# An alias that is configured but would not route.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.
backends-status-drained = mantenimiento

# Filas de backend de la SPA: interruptor de drenaje, carga por hora y borrado.
backends-drain-button = Drenar
backends-undrain-button = Reactivar
backends-requests-per-hour = { $count } sol./h
backends-delete-confirm = ¿Eliminar el backend { $name }? Aplica la topología después.
