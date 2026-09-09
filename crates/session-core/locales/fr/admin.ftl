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
admin-comfyui-reloaded = { $count } workflow(s) rechargé(s).
admin-comfyui-empty = Aucun workflow chargé — vérifiez le répertoire de contenu.
admin-defaults-model-aria = Modèle par défaut pour { $feature }
admin-defaults-set = Définir
admin-search-provider-none = Aucun
admin-add-overrides-heading = Ajouter des surcharges de modèle
admin-edit-model-heading = Modifier { $model }
admin-add-model = Ajouter…
admin-pricing-unit-label = Unité de tarification
admin-pricing-unit-mtok = par Mtok
admin-pricing-unit-ktok = par Ktok
admin-pricing-unit-kimgs = par 1000 images
admin-clear-overrides-confirm = Supprimer toutes les surcharges enregistrées pour { $model } ?
admin-users-col-email = E-mail
