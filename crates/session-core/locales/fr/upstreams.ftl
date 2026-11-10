# STATUS: llm-generated, unreviewed — pending native-speaker QA
# La surface « upstreams » : ce qu'est un pool, et les en-têtes du tableau.

upstreams-heading = Upstreams
upstreams-description = Les pools regroupent les backends par type et par stratégie de sélection. La santé, la charge et les modèles servis sont sondés en direct. Les modifications de topologie sont enregistrées en base et appliquées immédiatement au registre en cours.
upstreams-add-pool = Pool
upstreams-add-backend = Backend
upstreams-unassigned-heading = Non assignés
upstreams-unassigned-description = Backends non assignés à un pool. Ajoutez-en un à un pool pour y acheminer le trafic.
upstreams-empty = Aucun pool ni backend configuré pour l'instant. Ajoutez un pool ou un backend pour commencer.
upstreams-cancel = Annuler
upstreams-edit-backend = Modifier le backend
upstreams-comp-limits = limites
upstreams-edit-pool = Modifier le pool

# Le tableau concours : en-têtes de colonnes, vocabulaire des lampes, actions des lignes.
board-col-name = Backend
board-col-state = État
board-col-load = Charge
board-col-requests = Requêtes/h
board-summary-on-time = à l'heure
board-summary-degraded = en retard
board-summary-down = annulé
board-summary-maintenance = maintenance
board-on-time = À l'heure
board-on-time-detail = Sain, sert le trafic
board-saturated = Saturé
board-saturated-detail = { $inflight } sur { $max } requêtes en cours utilisées
board-down = En panne
board-down-detail = La sonde de santé échoue
board-maintenance = Maintenance
board-maintenance-detail = En pause — n'accepte pas de nouveau trafic
board-auth = Clé refusée
board-auth-detail = L'amont a refusé la clé API
board-no-probe = Pas de sonde
board-no-probe-detail = Pas encore sondé
board-models-unit = modèles
board-legend-on-time = à l'heure — sain, sert
board-legend-degraded = en retard — saturé ou problème de clé
board-legend-down = annulé — injoignable
board-legend-maintenance = maintenance — en pause
board-enabled = Sert
board-drained = En pause
board-edit = Modifier

board-detail-endpoint = Point de terminaison
board-detail-models = Modèles
board-detail-aliases = Alias
board-detail-activity = Trafic
board-key-none = aucune clé configurée
board-models-none = aucun découvert
board-models-more = +{ $count } de plus
board-pool-account = Compte
board-pool-managed = Ce pool est géré dans le compte OpenRouter ci-dessous.

board-filter-placeholder = Filtrer backends, modèles, alias…
board-filter-clear = Effacer
board-filter-empty = Aucun backend ni pool ne correspond à ce filtre.
board-edit-named = Modifier le backend { $name }
upstreams-saved = Enregistré — actif immédiatement.
board-model-open-catalog = Ouvrir ce modèle dans le catalogue
