# STATUS: llm-generated, unreviewed — pending native-speaker QA

backends-heading = Backends en amont

backends-status-down = hors service
backends-status-up = actif

backends-inflight-label = en cours { $load }

# Éditeur CRUD des backends (ajout/modification/suppression de backends stockés dans la topologie en base).
backends-apply-changes = Appliquer les modifications
backends-field-name = Nom
backends-field-base-url = URL de base
backends-field-pool = Pool
backends-field-pool-none = (aucun)
backends-save-backend = Enregistrer le backend
backends-add-backend = Ajouter un backend
backends-delete-backend = Supprimer

backends-field-api-key = Clé API
backends-field-api-key-keep = laisser vide pour conserver la clé actuelle

# An alias that is configured but would not route.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.
backends-status-drained = maintenance

# Lignes de backend de la SPA : bascule de drainage, charge horaire, suppression.
backends-drain-button = Drainer
backends-undrain-button = Réactiver
backends-requests-per-hour = { $count } req/h
backends-delete-confirm = Supprimer le backend { $name } ? Appliquez la topologie ensuite.
