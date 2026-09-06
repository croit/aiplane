# STATUS: llm-generated, unreviewed — pending native-speaker QA

pools-page-title = Pools en amont — LLM Gateway
pools-heading = Pools en amont
pools-description = Regroupez les backends en pools par type et stratégie de sélection. Les modifications sont enregistrées en base mais ne prennent effet qu'une fois que vous cliquez sur « Appliquer les modifications ».

pools-fallbacks-heading = Replis pour modèles inconnus
pools-fallbacks-description = Lorsqu'une requête nomme un modèle que la gateway n'a jamais rencontré, ce modèle est substitué pour ce type. Vide = l'absence renvoie 404.

pools-add-heading = Ajouter un pool
pools-field-name = Nom
pools-field-kind = Type
pools-field-strategy = Stratégie
pools-field-fallback-offline = Modèle de repli hors ligne
pools-field-fallback-offline-placeholder = servi lorsque tous les backends sont hors service
pools-field-models = Modèles servis (liste d'autorisation, séparés par des virgules)
pools-field-models-hint = Si défini, seuls ces ids d'un backend avec sonde /models sont servis — les autres sont affichés barrés. Vide = servir tout ce que le backend signale.
pools-field-voices = Voix (lang=voice par ligne)
pools-field-offer-voices = Voix sélectionnables (une par ligne, au choix de l'utilisateur)
pools-field-backends = Backends
pools-no-backends = Aucun backend défini pour l'instant. Ajoutez-en un d'abord sur la page Backends.
pools-field-gdpr = Conforme au GDPR
pools-field-nda = Couvert par NDA
pools-field-enforce-limits = Appliquer les limites de débit et les quotas
pools-save-pool = Enregistrer le pool
pools-add-pool = Ajouter un pool
pools-delete-pool = Supprimer

pools-error-name-required = le nom du pool est requis
pools-error-invalid-kind = type de pool invalide `{ $kind }`
pools-saved = pool `{ $name }` enregistré — cliquez sur « Appliquer les modifications » pour recharger
pools-deleted = pool `{ $name }` supprimé — cliquez sur « Appliquer les modifications » pour recharger
pools-fallback-saved = repli { $kind } défini sur `{ $model }`
pools-fallback-cleared = repli { $kind } effacé

pools-field-allowed-groups = Groupes autorisÃ©s
pools-field-allowed-groups-hint = Groupes du gateway (sÃ©parÃ©s par des virgules) autorisÃ©s Ã  voir et utiliser les modÃ¨les de ce pool. Vide = tout le monde. Les admins ont toujours accÃ¨s. GÃ©rez les groupes dans Admin â Groupes.

# Duplicate-name guard on the Add-pool form.
pools-error-name-exists = un pool nommé `{ $name }` existe déjà — cliquez de nouveau sur « Ajouter un pool » pour l'écraser, ou changez le nom
pools-overwrite-hint = Ce nom existe déjà. Enregistrer à nouveau ÉCRASE le pool existant — ses backends, modèles, voix et indicateurs de conformité. Changez le nom pour créer un pool distinct.
# Why the strategy choice matters for self-hosted replicas.
pools-field-strategy-hint = prefix_affinity garde une conversation sur la réplique qui détient déjà son cache KV (idéal pour le trafic chat/agent réparti sur plusieurs GPU — les autres alternent les tours entre répliques et repaient un prefill complet chaque fois), tout en répartissant si un backend est réellement plus chargé. least_inflight équilibre selon la charge actuelle ; round_robin tourne selon le poids.
# What the pool advertises, and how many replicas serve each name (U6/U7).
upstreams-coverage-heading = Ce que voient les clients
upstreams-coverage-hint = Exactement les noms que GET /v1/models renvoie pour ce pool, chacun avec le nombre de backends capables de le servir maintenant. Tout ce qui n'est pas complet signifie qu'une partie de votre matériel est inactive pour ce nom.
upstreams-coverage-full-title = Tous les backends de ce pool servent ce nom.
upstreams-coverage-partial-title = Seuls certains backends servent ce nom : les requêtes n'utilisent qu'une partie du pool, le reste reste inactif. Généralement un alias dont la cible ne correspond pas à ce qu'annonce un backend.
upstreams-coverage-none-title = Aucun backend ne peut servir ce nom actuellement. Les requêtes reçoivent une erreur de panne temporaire.

# The per-pool problem summary (U8).
upstreams-problems-heading = Ce pool ne fonctionne pas complètement
upstreams-problem-no-backends = Aucun backend assigné — rien dans ce pool ne peut servir une requête.
upstreams-problem-all-drained = Tous les backends sont vidés pour maintenance : rien n'est routé ici.
upstreams-problem-all-down = Aucun backend disponible : les requêtes attendent un retour puis reçoivent une erreur de panne temporaire.
upstreams-problem-auth = Identifiants refusés par : { $backends }. La découverte des modèles y est désactivée, ils n'annoncent rien.
upstreams-problem-no-models = Aucun modèle annoncé par : { $backends }. Rien n'y est routé, et un alias nu n'y a rien à quoi se lier.
upstreams-problem-broken-aliases = Alias qui ne routent nulle part : { $aliases }. Chacun pointe vers un modèle que son backend ne sert pas, ou n'a rien à quoi se lier.
upstreams-problem-partial-coverage = Servis par une partie seulement du pool : { $models }. Les requêtes sur ces noms utilisent moins de répliques que vous n'en avez.
upstreams-problem-unserved-allowlist = Listés sous Modèles mais servis par personne : { $models }.
upstreams-problem-missing-backends = Backends assignés qui n'existent plus : { $backends }.
# Live name-clash note on the add form (U9).
pools-name-taken = Un pool portant ce nom existe déjà — enregistrer le remplacerait, backends et modèles compris. Choisissez un autre nom pour créer un nouveau pool.
# The apply diff (U10).
upstreams-apply-diff-summary = Voir ce que l'application va changer
upstreams-diff-pool-added = le nouveau pool { $pool } entre en service
upstreams-diff-pool-removed = le pool { $pool } cesse de servir
upstreams-diff-pool-kind = pool { $pool } : type { $from } → { $to }
upstreams-diff-pool-strategy = pool { $pool } : stratégie { $from } → { $to }
upstreams-diff-backend-joins = { $backend } rejoint le pool { $pool } et commence à recevoir du trafic
upstreams-diff-backend-leaves = { $backend } quitte le pool { $pool } et ne reçoit plus de trafic
upstreams-diff-backend-url = { $backend } : URL de base { $from } → { $to } (ses modèles découverts sont re-sondés)
upstreams-diff-backend-limits = { $backend } : poids { $weight }, max en vol { $inflight }
upstreams-diff-backend-health-path = { $backend } : chemin de santé → { $to }
