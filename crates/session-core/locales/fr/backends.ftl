# STATUS: llm-generated, unreviewed — pending native-speaker QA


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
backends-delete-backend = Supprimer

backends-field-api-key = Clé API
backends-field-api-key-keep = laisser vide pour conserver la clé actuelle
backends-field-api-key-stored = Clé enregistrée : { $hint }

# An alias that is configured but would not route.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.
backends-status-drained = maintenance

# Lignes de backend de la SPA : bascule de drainage, charge horaire, suppression.
backends-add-heading = Ajouter un backend
backends-name-taken = Un backend portant ce nom existe déjà — enregistrer l'écraserait, URL de base, clé, modèles et pool compris. Choisissez un autre nom pour ajouter un second backend.
backends-field-api-key-placeholder = Clé API (stockée chiffrée)
backends-field-api-key-env = Variable d'env de la clé API
backends-field-health-path = Chemin de santé
backends-field-pool-hint = Affecte ce backend à un pool. Un backend présent dans plusieurs pools est réduit à celui choisi ici.
backends-field-weight = Poids
backends-field-max-inflight = Max en cours
backends-field-models = Modèles (séparés par des virgules ; vide = découverte via /models)
backends-field-aliases = Alias (name=target par ligne)
backends-field-extra-headers = En-têtes supplémentaires (nom : valeur par ligne)
backends-field-extra-headers-hint = Envoyé avec chaque requête vers ce backend, après la clé API. Par exemple anthropic-version: 2023-06-01, que Anthropic exige.
backends-field-supports-edit = Prend en charge l'édition d'images
backends-status-saturated = saturé
backends-auth-failed-title = L'upstream a refusé les identifiants de la sonde de santé (401/403), la découverte des modèles est donc désactivée — rien de nouveau ne peut devenir routable via ce backend. Vérifiez la clé API ; si elle vient d'une variable d'environnement, vérifiez qu'elle est bien définie.
backends-auth-failed = clé refusée
backends-no-models-title = Ce backend n'annonce aucun modèle : rien n'y est routé et un alias nu n'a rien à quoi se lier. Le plus souvent une sonde qui n'a jamais renvoyé de données.
backends-no-models = aucun modèle proposé
backends-key-env-badge = clé : env { $var }
backends-key-env-unset-badge = env { $var } NON DÉFINIE
backends-enabled-hint = Désactivez pour vider ce backend en vue d'une maintenance. Effet immédiat — pas besoin d'« Appliquer les modifications ». Ses modèles restent connus : les autres backends du pool prennent le relais et les clients voient une panne temporaire, jamais « modèle introuvable ».
backends-enabled-label = Accepte du trafic
backends-activity-summary = 15 min { $m15 } · 30 min { $m30 } · 60 min { $m60 }
backends-aliases-label = alias :
backends-alias-target-title = alias → { $target }
backends-alias-disabled-title = alias simple désactivé — ce backend propose plusieurs modèles ; indiquez-lui une cible explicite (formulaire de correspondance)
backends-alias-disabled-label = { $name } (désactivé)
backends-fallback-offline-title = fallback_offline : utilisé lorsque tous les backends d'un modèle connu de ce pool sont hors service
backends-fallback-offline-badge = hors ligne ↩ { $model }
backends-pool-empty = Aucun backend dans ce pool.

backends-test-button = Tester la connexion
backends-test-hint = Appelle cette URL avec les identifiants ci-dessus. Rien n'est enregistré.
backends-test-insert-hint = Ids de modèles annoncés — cliquez pour compléter la ligne d'alias sous le curseur :
backends-test-ok = Joignable, authentifié ({ $source }), { $count } modèles découverts.
backends-test-ok-no-models = Joignable et authentifié ({ $source }), mais la réponse n'est ni une liste /models OpenAI ni TypeSafe : la découverte ne peut pas la lire. Ce backend ne pourra servir que les ids listés sous « Modèles ».
backends-test-auth-failed = Refusé avec HTTP { $status } : identifiant rejeté ({ $source }). La découverte des modèles reste désactivée tant que ce n'est pas corrigé, et le backend n'annonce donc rien.
backends-test-http-error = { $url } a répondu HTTP { $status }.
backends-test-upstream-said = Réponse de l'upstream : { $detail }
backends-test-unreachable = Impossible de joindre { $url } : { $err }
backends-test-timeout = { $url } n'a pas répondu en { $secs } s.
backends-test-key-typed = avec la clé saisie ci-dessus
backends-test-key-stored = avec la clé stockée
backends-test-key-env = depuis env { $var }
backends-test-key-env-unset = env { $var } N'EST PAS DÉFINIE — la requête est partie sans identifiant
backends-test-key-none = aucun identifiant envoyé
backends-error-base-url-required = l'URL de base est requise

backends-parallel-mismatch = le serveur en traite { $parallel } à la fois
backends-detect-profile = identifié : { $profile }

# OpenRouter catalog: one backend, one key, the account's models, and a
# purchase-price ceiling per selected model.
openrouter-heading = OpenRouter
openrouter-description = One OpenRouter connection. Pick the models this gateway may use and the maximum you will pay for each; a model with no provider under its ceiling is not offered.
openrouter-connected = Connected
openrouter-key-label = Key
openrouter-credits = Credits
openrouter-refresh = Refresh prices
openrouter-backend = Backend
openrouter-pool = Pool
openrouter-search = Search models
openrouter-select-all = Select all
openrouter-deselect-all = Deselect all
openrouter-selected-count = { $count } selected
openrouter-model = Model
openrouter-price = List price
openrouter-context = Context
openrouter-tools = Tools
openrouter-ceiling-prompt = Max input $/1M
openrouter-ceiling-completion = Max output $/1M
openrouter-strategy = Provider strategy
openrouter-strategy-cheapest = Cheapest
openrouter-strategy-fastest = Fastest
openrouter-strategy-balanced = Balanced
openrouter-min-throughput = Min tokens/s
openrouter-available = Available
openrouter-over-ceiling = Over ceiling
openrouter-save = Save selection
openrouter-saved = Selection saved.
openrouter-no-models = No models match this filter.
openrouter-error = OpenRouter: { $message }
openrouter-select-model-first = Select at least one model.
openrouter-endpoints = Providers
openrouter-provider = Provider
openrouter-throughput = Tokens/s
openrouter-uptime = Uptime
openrouter-add-backend = Add OpenRouter backend
openrouter-provider-field = Provider
openrouter-provider-openai = OpenAI-compatible
openrouter-provider-openrouter = OpenRouter
openrouter-provider-hint = OpenRouter reads the account catalog and enforces the per-model price ceiling on every request. Other providers are plain OpenAI-compatible HTTP.
openrouter-ceiling-required = Every selected model needs both price ceilings.
openrouter-pool-empty = This backend is not in a pool yet. Add it to a chat, embedding, speech, transcription, image, rerank or System One pool first.
openrouter-mixed-pool = This pool also contains { $backends }. A pool has one model list, so saving here pins theirs too; put OpenRouter in its own pool.
openrouter-max-price = Max $/1M
openrouter-voices = Voix
