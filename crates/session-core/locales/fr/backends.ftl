# STATUS: llm-generated, unreviewed — pending native-speaker QA

backends-page-title = Backends en amont — LLM Gateway
backends-heading = Backends en amont
backends-description-prefix = Vue en direct des pools en amont configurés — état, charge en cours par rapport à la limite de chaque backend, et les modèles que chacun propose actuellement. Lecture seule : le routage dépend entièrement de ce que les backends signalent via leur
backends-description-suffix = sonde.
backends-summary = { $total } backends · { $healthy } opérationnels · { $down } hors service
backends-unknown-fallback-prefix = Repli pour modèle inconnu —
backends-empty-prefix = Aucun pool en amont configuré. Ajoutez un bloc
backends-empty-suffix = à gateway.toml et redémarrez.

backends-fallback-offline-title = fallback_offline : utilisé lorsque tous les backends d'un modèle connu de ce pool sont hors service
backends-fallback-offline-badge = hors ligne ↩ { $model }
backends-pool-empty = Aucun backend dans ce pool.

backends-status-down = hors service
backends-status-saturated = saturé
backends-status-up = actif

backends-inflight-label = en cours { $load }
backends-activity-summary = 15 min { $m15 } · 30 min { $m30 } · 60 min { $m60 }
backends-no-models = aucun modèle proposé
backends-aliases-label = alias :

backends-alias-target-title = alias → { $target }
backends-alias-disabled-label = { $name } (désactivé)
backends-alias-disabled-title = alias simple désactivé — ce backend propose plusieurs modèles ; indiquez-lui une cible explicite (formulaire de correspondance)
backends-alias-bare-title = alias → modèle de ce backend

# Éditeur CRUD des backends (ajout/modification/suppression de backends stockés dans la topologie en base).
backends-manage-heading = Gérer les backends
backends-manage-description = Ajoutez, modifiez ou supprimez des backends en amont. Les modifications sont enregistrées en base mais ne prennent effet qu'une fois que vous cliquez sur « Appliquer les modifications ».
backends-apply-changes = Appliquer les modifications
backends-add-heading = Ajouter un backend
backends-field-name = Nom
backends-field-base-url = URL de base
backends-field-api-key-env = Variable d'env de la clé API
backends-field-health-path = Chemin de santé
backends-field-weight = Poids
backends-field-max-inflight = Max en cours
backends-field-pool = Pool
backends-field-pool-none = (aucun)
backends-field-pool-hint = Affecte ce backend à un pool. Un backend présent dans plusieurs pools est réduit à celui choisi ici.
backends-field-models = Modèles (séparés par des virgules)
backends-field-aliases = Alias (name=target par ligne)
backends-field-probe-models = Découvrir les modèles via la sonde /models
backends-field-supports-edit = Prend en charge l'édition d'images
backends-save-backend = Enregistrer le backend
backends-add-backend = Ajouter un backend
backends-delete-backend = Supprimer
backends-error-name-required = le nom du backend est requis
backends-error-base-url-required = l'URL de base est requise
backends-saved = backend `{ $name }` enregistré — cliquez sur « Appliquer les modifications » pour recharger
backends-deleted = backend `{ $name }` supprimé — cliquez sur « Appliquer les modifications » pour recharger

backends-field-api-key = Clé API
backends-field-api-key-placeholder = Clé API (stockée chiffrée)
backends-field-api-key-keep = laisser vide pour conserver la clé actuelle

# Duplicate-name guard on the Add-backend form.
backends-error-name-exists = un backend nommé `{ $name }` existe déjà — cliquez de nouveau sur « Ajouter un backend » pour l'écraser, ou changez le nom
backends-overwrite-hint = Ce nom existe déjà. Enregistrer à nouveau ÉCRASE le backend existant — son URL de base, sa clé API, ses modèles, ses alias et son pool. Changez le nom pour ajouter un second backend.

# An alias that is configured but would not route.
backends-alias-unresolved-title = CASSÉ : cet alias pointe vers `{ $target }`, que ce backend ne sert pas — les requêtes échouent.
backends-alias-nothing-title = CASSÉ : ce backend n'annonce aucun modèle, un alias nu n'a rien à quoi se lier.
backends-alias-serves = Il sert : { $models }
backends-alias-serves-nothing = Il ne sert actuellement aucun modèle.
# Save-time check on the aliases textarea.
backends-alias-target-unknown = enregistré, mais ces cibles d'alias ne sont pas servies par ce backend : { $targets } — il sert { $models }. Ces alias ne routeront pas tant que la cible ne correspond pas exactement.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
backends-auth-failed = clé refusée
backends-auth-failed-title = L'upstream a refusé les identifiants de la sonde de santé (401/403), la découverte des modèles est donc désactivée — rien de nouveau ne peut devenir routable via ce backend. Vérifiez la clé API ; si elle vient d'une variable d'environnement, vérifiez qu'elle est bien définie.
backends-no-models-title = Ce backend n'annonce aucun modèle : rien n'y est routé et un alias nu n'a rien à quoi se lier. Le plus souvent une sonde qui n'a jamais renvoyé de données.
# Maintenance switch.
backends-enabled-label = Accepte du trafic
backends-enabled-hint = Désactivez pour vider ce backend en vue d'une maintenance. Effet immédiat — pas besoin d'« Appliquer les modifications ». Ses modèles restent connus : les autres backends du pool prennent le relais et les clients voient une panne temporaire, jamais « modèle introuvable ».
backends-enabled-on = le backend `{ $name }` accepte à nouveau du trafic
backends-enabled-off = backend `{ $name }` vidé pour maintenance — aucune nouvelle requête n'y sera routée
backends-status-drained = maintenance
backends-status-drained-title = Vidé pour maintenance : ce backend est joignable mais le routeur l'ignore. Réactivez « Accepte du trafic » pour le remettre en rotation.
# "Test connection": call the upstream with what is typed in the editor.
backends-test-button = Tester la connexion
backends-test-hint = Appelle cette URL avec les identifiants ci-dessus. Rien n'est enregistré.
backends-test-insert-hint = Ids de modèles annoncés — cliquez pour compléter la ligne d'alias sous le curseur :
backends-test-ok = Joignable, authentifié ({ $source }), { $count } modèles découverts.
backends-test-ok-no-models = Joignable et authentifié ({ $source }), mais la réponse n'est pas une enveloppe /models OpenAI : la découverte ne peut pas la lire. Ce backend ne pourra servir que les ids listés sous « Modèles ».
backends-test-auth-failed = Refusé avec HTTP { $status } : identifiant rejeté ({ $source }). La découverte des modèles reste désactivée tant que ce n'est pas corrigé, et le backend n'annonce donc rien.
backends-test-http-error = { $url } a répondu HTTP { $status }.
backends-test-unreachable = Impossible de joindre { $url } : { $err }
backends-test-timeout = { $url } n'a pas répondu en { $secs } s.
backends-test-key-typed = avec la clé saisie ci-dessus
backends-test-key-stored = avec la clé stockée
backends-test-key-env = depuis env { $var }
backends-test-key-env-unset = env { $var } N'EST PAS DÉFINIE — la requête est partie sans identifiant
backends-test-key-none = aucun identifiant envoyé
# Where the API key comes from (U5).
backends-key-env-badge = clé : env { $var }
backends-key-env-title = Ce backend n'a pas de clé stockée ; il en lit une depuis cette variable d'environnement, actuellement définie.
backends-key-env-unset-badge = env { $var } NON DÉFINIE
backends-key-env-unset-title = Ce backend n'a pas de clé stockée et la variable d'environnement qu'il nomme n'est pas définie dans le processus du gateway : aucune identification n'est envoyée. Si l'upstream en exige une, chaque sonde reçoit 401, la découverte des modèles reste désactivée et le backend n'annonce rien. Saisissez la clé dans le champ « Clé API », ou définissez la variable et redémarrez.
# Live name-clash note on the add form (U9).
backends-name-taken = Un backend portant ce nom existe déjà — enregistrer l'écraserait, URL de base, clé, modèles et pool compris. Choisissez un autre nom pour ajouter un second backend.

# Lignes de backend de la SPA : bascule de drainage, charge horaire, suppression.
backends-drain-button = Drainer
backends-undrain-button = Réactiver
backends-requests-per-hour = { $count } req/h
backends-delete-confirm = Supprimer le backend { $name } ? Appliquez la topologie ensuite.
