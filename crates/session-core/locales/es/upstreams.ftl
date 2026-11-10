# STATUS: llm-generated, unreviewed — pending native-speaker QA

upstreams-apply-count = cambios sin aplicar
upstreams-apply-note = — el registro en tiempo de ejecución aún sirve la topología anterior.

# Aviso tras aplicar la SPA la topología pendiente.
upstreams-applied = Topología aplicada.

# La superficie de upstreams: qué es un pool y las cabeceras del tablero.

upstreams-heading = Upstreams
upstreams-description = Los pools agrupan backends por clase y estrategia de selección. La salud, la carga y los modelos servidos se sondean en vivo. Los cambios de topología se guardan en la base de datos y surten efecto al aplicar los cambios.
upstreams-add-pool = Pool
upstreams-add-backend = Backend
upstreams-unassigned-heading = Sin asignar
upstreams-unassigned-description = Backends no asignados a ningún pool. Añádelos a un pool para dirigirles tráfico.
upstreams-empty = Aún no hay pools ni backends configurados. Añade un pool o un backend para empezar.
upstreams-cancel = Cancelar
upstreams-edit-backend = Editar backend
upstreams-comp-limits = límites
upstreams-edit-pool = Editar pool

# El tablero de andén: encabezados de columna, vocabulario de las luces, acciones por fila.
board-col-name = Backend
board-col-state = Estado
board-col-load = Carga
board-col-requests = Peticiones/h
board-summary-on-time = a tiempo
board-summary-degraded = retrasado
board-summary-down = cancelado
board-summary-maintenance = mantenimiento
board-on-time = A tiempo
board-on-time-detail = Sano, sirve tráfico
board-saturated = Saturado
board-saturated-detail = { $inflight } de { $max } peticiones en curso en uso
board-down = Caído
board-down-detail = La sonda de salud falla
board-maintenance = Mantenimiento
board-maintenance-detail = En pausa — no admite tráfico nuevo
board-auth = Clave rechazada
board-auth-detail = El upstream rechazó la clave API
board-no-probe = Sin sonda
board-no-probe-detail = Aún no sondeado
board-models-unit = modelos
board-legend-on-time = a tiempo — sano, sirve
board-legend-degraded = retrasado — saturado o problema de clave
board-legend-down = cancelado — inalcanzable
board-legend-maintenance = mantenimiento — en pausa
board-enabled = Sirviendo
board-drained = En pausa
board-edit = Editar

board-detail-endpoint = Endpoint
board-detail-models = Modelos
board-detail-aliases = Alias
board-detail-activity = Tráfico
board-key-none = sin clave configurada
board-models-none = ninguno descubierto
board-models-more = +{ $count } más
board-pool-account = Cuenta
board-pool-managed = Este pool se gestiona en la cuenta de OpenRouter de abajo.
