# STATUS: llm-generated, unreviewed — pending native-speaker QA

rag-heading = Collections RAG

# Toasts — collection CRUD
rag-toast-vanished = La collection a disparu après l'enregistrement.

# Toasts — refs / sources
rag-toast-bulk-queued-skipped = { $added } source(s) mise(s) en file d'attente ; { $skipped } doublon(s) ignoré(s).
rag-toast-bulk-queued = Indexation de { $added } source(s) mise en file d'attente.
rag-toast-reindex-queued-ref = Réindexation de `{ $ref }` mise en file d'attente.

# Status badges
rag-status-pending = en attente
rag-status-cloning = clonage
rag-status-indexing = indexation
rag-status-ready = prêt
rag-status-error = erreur

# Collection row
rag-button-edit = Modifier
rag-button-add-source = Ajouter une source
rag-button-add-bulk = Ajouter des sources (en masse)

# Ref / source row
rag-badge-primary = principale
rag-button-reindex = Réindexer
rag-button-set-primary = Définir comme principale
rag-button-remove = Supprimer

# Inline per-source editor
rag-label-branch-tag = Branche / tag
rag-button-cancel = Annuler

# Create-collection form
rag-label-name = Nom
rag-label-chunk-size = Taille du chunk
rag-label-chunk-overlap = Chevauchement du chunk

# Edit-collection form
rag-label-description = Description

# Embedding model field
rag-label-embedding-model = Modèle d'embedding

# Sélecteur de source + identifiants du fournisseur (rag_source.rs). Les
# libellés des champs viennent du fournisseur et ne sont pas traduits.
rag-label-source-kind = Source
rag-source-unknown-kind = Type de source inconnu.
rag-source-test-button = Tester la connexion
rag-source-test-ok = Connecté en tant que `{ $account }`. { $entries } élément(s) dans le dossier configuré.
rag-source-test-ok-plain = Connecté. { $entries } élément(s) dans le dossier configuré.
rag-source-test-failed = Source injoignable : { $error }
rag-source-detected = Détecté : { $server }

rag-label-profile = Champs de document
rag-option-profile-none = Aucun — indexer seulement le texte

# Hook de synchronisation — un déclencheur entrant qui resynchronise une collection.
rag-button-sync-token = URL de sync
rag-badge-sync-hook = hook de sync

# Browser consent for an OAuth source (Google Drive).
rag-source-consent-save-first = Enregistrez d'abord la collection avec son ID client et son secret, puis connectez-la pour accorder l'accès.
rag-oauth-lookup-failed = Impossible de lire la collection.
rag-oauth-not-oauth = Ce type de source ne se connecte pas dans le navigateur.
rag-oauth-no-client = Enregistrez d'abord l'ID client et le secret OAuth sur la collection.
rag-oauth-bad-authorize-url = Impossible de construire l'URL d'autorisation du fournisseur.
rag-oauth-start-failed = Impossible de démarrer l'autorisation.
rag-oauth-callback-missing = Le code ou l'état manquait dans la réponse du fournisseur.
rag-oauth-expired = Cette autorisation a expiré ou a déjà été utilisée. Recommencez.
rag-oauth-provider-refused = Le fournisseur a refusé l'autorisation : { $error }
rag-oauth-exchange-failed = L'échange du code d'autorisation a échoué : { $error }
rag-oauth-no-refresh-token = Le fournisseur n'a renvoyé aucun jeton de rafraîchissement ; l'indexation autonome serait impossible. Révoquez l'accès de la passerelle dans votre compte fournisseur puis reconnectez.
rag-oauth-store-failed = Impossible d'enregistrer les identifiants.

# Gestionnaire de collections de la SPA : formulaire de création, carte d'URL de
# synchronisation, lignes de source et les confirmations propres à la SPA.
rag-button-new-collection = Nouvelle collection
rag-label-git-url = URL Git
rag-source-testing = Test en cours…
rag-button-create = Créer
rag-button-rebuild = Reconstruire
rag-sync-url-heading = URL de synchronisation — affichée une seule fois
rag-sync-token-confirm = Générer une nouvelle URL de synchronisation ? L'ancienne cessera de fonctionner.
rag-delete-collection-confirm = Supprimer la collection { $name } et son index ?
rag-remove-source-confirm = Retirer la source { $source } ?
rag-toast-rebuild-queued = Reconstruction complète demandée.
rag-ref-indexed-at = indexé le { $date }
rag-no-sources = Aucune source — cette collection n'indexe rien tant qu'aucune n'est ajoutée.
rag-add-sources-hint = Une source par ligne ; ajoutez { $at } pour remplacer la réf. { $ref } de cette collection.
