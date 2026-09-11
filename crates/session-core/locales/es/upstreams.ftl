# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/upstreams.rs` — la página
# combinada `/admin/upstreams` (pools + backends).

upstreams-apply-count = cambios sin aplicar
upstreams-apply-note = — el registro en tiempo de ejecución aún sirve la topología anterior.

upstreams-backend-pending = pendiente

# Aviso tras aplicar la SPA la topología pendiente.
upstreams-applied = Topología aplicada.
upstreams-heading = Upstreams
upstreams-description = Los pools agrupan backends por clase y estrategia de selección. La salud, la carga y los modelos servidos se sondean en vivo. Los cambios de topología se guardan en la base de datos y surten efecto al aplicar los cambios.
upstreams-add-pool = Pool
upstreams-add-backend = Backend
upstreams-unassigned-heading = Sin asignar
upstreams-unassigned-description = Backends no asignados a ningún pool. Añádelos a un pool para dirigirles tráfico.
upstreams-empty = Aún no hay pools ni backends configurados. Añade un pool o un backend para empezar.
upstreams-delete-confirm = ¿Eliminar de verdad?
upstreams-cancel = Cancelar
upstreams-model-withheld-title = Descubierto vía /models pero retenido por la lista de modelos de este pool: no se sirve ni se anuncia.
upstreams-models-inactive-hide = ocultar
upstreams-models-inactive-pill = +{ $count } inactivos
upstreams-edit-backend = Editar backend
upstreams-comp-gdpr = RGPD
upstreams-comp-nda = NDA
upstreams-comp-limits = límites
upstreams-edit-pool = Editar pool
