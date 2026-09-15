# Strings owned by `gateway/src/rama_server/pages/admin.rs` — la page
# `/admin/models`.

admin-heading = Modèles

admin-col-model = Modèle

admin-not-configured = non configuré

admin-badge-ctx = CTX

admin-save-model = Enregistrer le modèle
admin-edit-model = Modifier
admin-edit-model-page-title = Modifier le modèle
admin-model-not-found = Modèle introuvable. Il n'est peut-être plus proposé par aucun backend.
admin-clear-overrides = Effacer tous les réglages
admin-cancel = Annuler

admin-toml-defaults-label = Valeurs d'échantillonnage (TOML)

# Tarifs par modèle pour la comptabilité des coûts (prix par 1 M de jetons, entrée / sortie).
admin-price-in-label = Prix ent
admin-price-out-label = Prix sor
admin-price-in-placeholder = sans tarif
admin-price-out-placeholder = sans tarif

# Fenêtre de contexte (pilote la compaction automatique).
admin-context-window-full-label = Fenêtre de contexte (jetons)
admin-context-window-placeholder = défaut

# Modèles par défaut par fonctionnalité.
admin-defaults-heading = Modèles par défaut

# Capacités du modèle (tri-état) + modèles de repli.
admin-cap-tools = Outils

# Backend de recherche web (outil `search_web`).
admin-search-heading = Recherche web
admin-search-provider-label = Fournisseur
admin-search-provider-searxng = SearXNG (auto-hébergé)
admin-search-provider-brave = Brave Search API
admin-search-searxng-url-label = URL de base SearXNG
admin-search-searxng-url-placeholder = https://searxng.example.com
admin-search-brave-key-label = Clé d'API Brave
admin-search-brave-key-placeholder = laisser vide pour conserver la clé actuelle
admin-search-save = Enregistrer la recherche web
admin-search-saved = paramètres de recherche web enregistrés

# ─── SPA d'administration SvelteKit ──────────────────────────────────────────
# Le garde-fou de rôle du shell admin, le catalogue de workflows /admin/comfyui
# et les parties de l'éditeur de modèles absentes de l'ancienne page.

admin-needs-admin-role = Ces pages nécessitent le rôle administrateur.
admin-overwrite-existing = écraser l'existant
admin-comfyui-reload = Recharger le catalogue
admin-comfyui-reloaded = Catalogue rechargé — { $count } workflow(s) chargé(s).
admin-comfyui-empty = Aucun workflow chargé — vérifiez le répertoire de contenu.
admin-comfyui-heading = Catalogue de workflows ComfyUI
admin-comfyui-page-title = ComfyUI — Catalogue de workflows
admin-comfyui-intro = Worker ComfyUI sans interface. La passerelle expose chaque workflow chargé comme outil comfyui_<id> que le modèle peut appeler. Les utilisateurs ne voient jamais ComfyUI.
admin-comfyui-not-configured = Non configuré
admin-comfyui-not-configured-help = Activez ComfyUI dans les paramètres, définissez son URL et son répertoire de workflows, puis redémarrez la passerelle.
admin-comfyui-operator-config = Configuration opérateur
admin-comfyui-worker-url = URL de base du worker
admin-comfyui-content-directory = Répertoire de contenu
admin-comfyui-timeout = Délai du workflow
admin-comfyui-poll-interval = Intervalle d’interrogation de la file
admin-comfyui-config-help = Le répertoire de contenu est géré par l’opérateur et ne fait pas partie du dépôt public. Modifiez-y les manifestes puis cliquez sur Recharger — aucun redémarrage requis.
admin-comfyui-loaded-workflows = Workflows chargés
admin-comfyui-node = nœud { $id }
admin-comfyui-parameters = Paramètres
admin-comfyui-required = requis
admin-comfyui-recent-jobs = Tâches récentes
admin-comfyui-reloaded-skipped = Catalogue rechargé — { $count } workflow(s) chargé(s), { $skipped } ignoré(s).
admin-comfyui-max-concurrent = Tâches simultanées
admin-comfyui-tab-workflows = Workflows
admin-comfyui-tab-jobs = Exécutions
admin-comfyui-jobs-page-title = ComfyUI — Exécutions
admin-comfyui-jobs-intro = Tous les workflows récemment exécutés par les modèles, du plus récent au plus ancien : ce qui a été produit, la durée et la conversation à l'origine de la demande.
admin-comfyui-jobs-window = Les { $count } exécutions les plus récentes enregistrées par la passerelle.
admin-comfyui-jobs-empty = Aucune exécution enregistrée pour l'instant.
admin-comfyui-worker-status = Worker
admin-comfyui-worker-reachable = Joignable
admin-comfyui-worker-unreachable = Injoignable
admin-comfyui-worker-checking = Vérification…
admin-comfyui-worker-queue = { $running } en cours · { $pending } en file
admin-comfyui-worker-software = ComfyUI { $version } · Python { $python } · PyTorch { $torch }
admin-comfyui-worker-vram = { $free } libres sur { $total }
admin-comfyui-search-placeholder = Rechercher workflows et paramètres
admin-comfyui-search-empty = Aucun workflow ne correspond à cette recherche.
admin-comfyui-filename-prefix = Préfixe de sortie
admin-comfyui-required-count = { $required } sur { $total } obligatoires
admin-comfyui-no-params = Ce workflow n'accepte aucun paramètre.
admin-comfyui-detail-empty = Choisissez un workflow pour voir le contrat vu par le modèle.
admin-comfyui-param-column = Paramètre
admin-comfyui-param-description-column = Description
admin-comfyui-filter-all = Toutes
admin-comfyui-filter-completed = Terminées
admin-comfyui-filter-pending = En attente
admin-comfyui-filter-failed = Échouées
admin-comfyui-stats-heading = Fiabilité par workflow
admin-comfyui-col-workflow = Workflow
admin-comfyui-col-runs = Exécutions
admin-comfyui-col-failed = Échecs
admin-comfyui-col-median = Médiane
admin-comfyui-col-status = Statut
admin-comfyui-col-duration = Durée
admin-comfyui-col-when = Démarrage
admin-comfyui-col-result = Résultat
admin-comfyui-job-open = Ouvrir la conversation
admin-comfyui-job-still-running = en cours
admin-comfyui-refresh = Actualiser
admin-clear-overrides-confirm = Supprimer toutes les surcharges enregistrées pour { $model } ?
admin-cap-no-fallback = (aucun)

admin-page-title = Modèles — LLM Gateway

admin-intro-prefix = Réglages par modèle — tarifs, fenêtre de contexte, raisonnement, capacités et valeurs d'échantillonnage — appliqués à

admin-intro-every = chaque

admin-intro-middle = requête pour ce modèle, quel que soit l'utilisateur ou le jeton, sauf si l'appelant définit la même valeur, laquelle

admin-intro-always-wins = l'emporte toujours

admin-intro-suffix = . Les modèles de chat, les alias et les autres types sont tous dans une seule liste.

admin-no-models = Aucun modèle annoncé pour l'instant. Dès qu'un backend en amont sera accessible, il apparaîtra ici.

admin-filter-placeholder = Filtrer les modèles…

admin-filter-all = Tous

admin-filter-chat = chat

admin-filter-other = autres types

admin-filter-aliases = alias

admin-filter-configured = configurés uniquement

admin-col-kind = Type

admin-col-price = Prix ent/sor

admin-col-context = Contexte

admin-col-reasoning = Raisonnement

admin-col-configured = Configuré

admin-value-default = défaut

admin-value-na = s/o

admin-alias-inherits = hérite des réglages de la cible

admin-reasoning-auto-resolved = Auto → { $style }

admin-badge-price = PRIX

admin-badge-budget = BUDGET

admin-badge-caps = CAPS

admin-badge-toml = TOML

admin-other-price-note = L'échantillonnage, le raisonnement et le contexte ne s'appliquent pas à ce type — seuls les tarifs, pour la comptabilité des coûts.

admin-toml-placeholder-header = # Clés courantes (vLLM/OpenAI) :

admin-reasoning-style-label = Style de raisonnement

admin-reasoning-style-aria = Style de raisonnement

admin-reasoning-auto = Auto

admin-reasoning-none = aucun

admin-reasoning-qwen = Qwen (vLLM)

admin-reasoning-openai = OpenAI

admin-reasoning-glm = GLM / z.AI

admin-reasoning-anthropic = Anthropic
admin-reasoning-ollama = Ollama

admin-effort-standard = Standard

admin-effort-deep = Approfondi

admin-effort-max = Max

admin-budget-placeholder = par défaut

admin-budget-hint = Nombre maximal de jetons de réflexion par niveau. Vide = valeur par défaut du backend (illimité). « Fast » désactive le raisonnement.

admin-effort-default-option = (par défaut)

admin-effort-hint = Effort de raisonnement par niveau. Vide = valeur par défaut intégrée. « Fast » désactive le raisonnement.

admin-saved-model = `{ $model }` enregistré — effet immédiat

admin-cleared-defaults = réglages effacés pour `{ $model }`

admin-price-label = { $cur }/{ $unit }

admin-price-unit-tokens = 1 M de tokens

admin-price-unit-images = image

admin-price-unit-characters = caractère

admin-price-unit-seconds = seconde

admin-alias-chip = alias

admin-defaults-intro = Choisissez le modèle présélectionné pour chaque fonctionnalité. Vide = le premier modèle disponible (comportement précédent).

admin-defaults-chat-label = Chat

admin-defaults-voice-label = Voix (transcription)

admin-defaults-image-label = Génération d'images

admin-defaults-embedding-label = Embedding (RAG)

admin-defaults-first-option = Premier disponible

admin-defaults-saved = modèle par défaut défini sur `{ $model }`

admin-defaults-cleared = modèle par défaut réinitialisé

admin-capabilities-heading = Capacités

admin-cap-vision = Vision

admin-cap-structured-output = Sortie structurée

admin-cap-audio-input = Entrée audio

admin-cap-pdf-input = Entrée PDF

admin-cap-parallel-tools = Outils en parallèle

admin-cap-unknown = Inconnu

admin-cap-enabled = Activé

admin-cap-disabled = Désactivé

admin-cap-fallback-vision = Repli pour la vision

admin-cap-fallback-tools = Repli pour les outils

admin-search-intro = Quel backend répond à l'outil `search_web` de l'assistant. SearXNG ne nécessite qu'une URL de base et ne coûte rien par requête si vous hébergez votre propre instance ; Brave nécessite une clé d'API. La clé est chiffrée au repos.

admin-search-brave-key-set = Une clé est enregistrée (chiffrée).

admin-search-brave-key-unset = Aucune clé enregistrée.

admin-search-brave-key-clear = Supprimer la clé enregistrée

# Provenance de la fenêtre de contexte.
admin-context-detected = Détecté : { $window } jetons
admin-context-unreported = Ce backend ne communique pas de fenêtre de contexte pour ce modèle. Saisissez-la ici ou vérifiez sur le serveur.
admin-context-exceeds-detected = Ce backend annonce { $window } jetons. Les valeurs supérieures ne sont pas compactées : le serveur les tronque silencieusement.
