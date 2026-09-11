# Strings owned by `gateway/src/rama_server/pages/admin.rs` — la page
# `/admin/models`.

admin-heading = Modèles

admin-col-model = Modèle

admin-not-configured = non configuré

admin-badge-ctx = CTX

admin-save-model = Enregistrer le modèle
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
admin-comfyui-workflow-meta = Titre : { $title } · préfixe : { $prefix }
admin-comfyui-parameters = Paramètres
admin-comfyui-required = requis
admin-comfyui-recent-jobs = Tâches récentes
admin-comfyui-pending = { $count } en attente
admin-comfyui-job-meta = prompt : { $prompt } · créé : { $created }
admin-comfyui-job-completed = terminé : { $completed }
admin-comfyui-reloaded-skipped = Catalogue rechargé — { $count } workflow(s) chargé(s), { $skipped } ignoré(s).
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
